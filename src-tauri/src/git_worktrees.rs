use crate::commands::calculate_dir_size;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Remote bases worth checking beyond `origin/HEAD`. A repository whose default branch is
/// `main` routinely merges feature work into `development` instead, and comparing against
/// `origin/HEAD` alone reports every one of those branches as unmerged.
const REMOTE_BASE_CANDIDATES: [&str; 4] = [
    "origin/main",
    "origin/master",
    "origin/development",
    "origin/develop",
];

/// Local fallbacks, used only when the repository has no usable remote base at all.
/// A stale local `master` happily claims that everything is merged, so this is a last resort.
const LOCAL_BASE_CANDIDATES: [&str; 4] = ["main", "master", "development", "develop"];

/// Untracked files that are noise from the OS or a file watcher, never work worth protecting.
/// They still make `git worktree remove` refuse, which is why they are tracked separately
/// from real changes instead of being folded into `is_dirty`.
const JUNK_EXACT_NAMES: [&str; 4] = [".DS_Store", "Thumbs.db", "desktop.ini", ".Spotlight-V100"];
const JUNK_NAME_PREFIXES: [&str; 2] = [".watchman-cookie", "._"];

const STATE_MERGED: &str = "merged";
const STATE_SQUASHED: &str = "squashed";
const STATE_STALE: &str = "stale";
const DETACHED_BRANCH_LABEL: &str = "(detached)";

/// A fetch that waits on a credential prompt never returns; the timeout is the second half of
/// the defence, after `GIT_TERMINAL_PROMPT=0`.
const FETCH_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, PartialEq, Eq)]
struct WorktreeRecord {
    path: PathBuf,
    head: String,
    branch: Option<String>,
    is_detached: bool,
    is_bare: bool,
    lock_reason: Option<String>,
    prunable_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct MergedWorktree {
    pub path: String,
    pub branch: String,
    pub repository_path: String,
    pub repository_name: String,
    pub base_branch: String,
    pub size: u64,
    pub is_dirty: bool,
    pub has_ignored_files: bool,
    pub is_locked: bool,
    pub lock_reason: Option<String>,
    /// Commit the scan saw, so a removal can tell "someone committed here" from "not merged".
    pub head: String,
    /// `merged` | `squashed` | `stale`.
    pub state: String,
    pub is_detached: bool,
    pub junk_file_count: u32,
    pub stale_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeScanResult {
    pub worktrees: Vec<MergedWorktree>,
    pub total_size: u64,
    pub scan_path: String,
    pub warnings: Vec<String>,
    /// Non-failure notes: which bases each repository was compared against, and where that
    /// comparison had to fall back to a local branch. Rendered separately from `warnings`.
    pub diagnostics: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorktreeRemoval {
    pub repository_path: String,
    pub worktree_path: String,
    /// HEAD as of the scan. Optional so an older frontend still round-trips.
    #[serde(default)]
    pub head: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeDeleteResult {
    pub success: bool,
    pub path: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaseBranches {
    branches: Vec<String>,
    used_local_fallback: bool,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct WorktreeStatus {
    is_dirty: bool,
    has_ignored_files: bool,
    junk_file_count: u32,
}

/// Throwaway object directory for the squash-merge probe. `git commit-tree` writes a real
/// object, and pointing `GIT_OBJECT_DIRECTORY` here keeps those synthetic commits out of the
/// user's repository instead of leaving one dangling commit per worktree per scan.
struct ScratchObjectDir {
    path: PathBuf,
}

impl ScratchObjectDir {
    fn new() -> Option<Self> {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "node-modules-cleaner-squash-probe-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).ok()?;
        Some(Self { path })
    }
}

impl Drop for ScratchObjectDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn git_stdout(directory: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

fn parse_worktree_porcelain(output: &[u8]) -> Vec<WorktreeRecord> {
    let mut records = Vec::new();
    let mut current: Option<WorktreeRecord> = None;

    for field in output.split(|byte| *byte == 0) {
        if field.is_empty() {
            if let Some(record) = current.take() {
                records.push(record);
            }
            continue;
        }

        let value = String::from_utf8_lossy(field);
        if let Some(path) = value.strip_prefix("worktree ") {
            if let Some(record) = current.take() {
                records.push(record);
            }
            current = Some(WorktreeRecord {
                path: PathBuf::from(path),
                head: String::new(),
                branch: None,
                is_detached: false,
                is_bare: false,
                lock_reason: None,
                prunable_reason: None,
            });
        } else if let Some(record) = current.as_mut() {
            if let Some(head) = value.strip_prefix("HEAD ") {
                record.head = head.to_string();
            } else if let Some(branch) = value.strip_prefix("branch refs/heads/") {
                record.branch = Some(branch.to_string());
            } else if value == "detached" {
                record.is_detached = true;
            } else if value == "bare" {
                record.is_bare = true;
            } else if value == "locked" {
                record.lock_reason = Some(String::new());
            } else if let Some(reason) = value.strip_prefix("locked ") {
                record.lock_reason = Some(reason.to_string());
            } else if value == "prunable" {
                record.prunable_reason = Some(String::new());
            } else if let Some(reason) = value.strip_prefix("prunable ") {
                record.prunable_reason = Some(reason.to_string());
            }
        }
    }

    if let Some(record) = current {
        records.push(record);
    }

    records
}

fn reference_resolves(repository: &Path, reference: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["rev-parse", "--verify", "--quiet"])
        .arg(format!("{reference}^{{commit}}"))
        .output()
        .is_ok_and(|output| output.status.success())
}

fn local_branch_exists(repository: &Path, branch: &str) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["show-ref", "--verify", "--quiet"])
        .arg(format!("refs/heads/{branch}"))
        .output()
        .is_ok_and(|output| output.status.success())
}

/// The local name a base branch corresponds to, so a worktree sitting on `main` can be matched
/// against the base `origin/main`.
fn base_branch_local_name(base: &str) -> &str {
    base.strip_prefix("origin/").unwrap_or(base)
}

fn find_base_branches(repository: &Path) -> BaseBranches {
    let mut branches: Vec<String> = Vec::new();

    if let Some(origin_head) = git_stdout(
        repository,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        if reference_resolves(repository, &origin_head) {
            branches.push(origin_head);
        }
    }

    for candidate in REMOTE_BASE_CANDIDATES {
        if !branches.iter().any(|branch| branch == candidate) && reference_resolves(repository, candidate)
        {
            branches.push(candidate.to_string());
        }
    }

    if !branches.is_empty() {
        return BaseBranches {
            branches,
            used_local_fallback: false,
        };
    }

    for candidate in LOCAL_BASE_CANDIDATES {
        if local_branch_exists(repository, candidate) {
            branches.push(candidate.to_string());
        }
    }

    let used_local_fallback = !branches.is_empty();
    BaseBranches {
        branches,
        used_local_fallback,
    }
}

fn is_ancestor(repository: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["merge-base", "--is-ancestor", ancestor, descendant])
        .output()
        .map_err(|error| format!("Failed to run git merge-base: {error}"))?;

    match output.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        code => {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(format!(
                "git merge-base failed{}: {}",
                code.map(|value| format!(" with exit code {value}"))
                    .unwrap_or_default(),
                if detail.is_empty() {
                    "unknown Git error".to_string()
                } else {
                    detail
                }
            ))
        }
    }
}

/// Is the branch's content already in the base even though its history is not?
///
/// A squash merge — GitHub's default — never makes the branch an ancestor of the base, so
/// `merge-base --is-ancestor` reports those branches as open forever. Replaying the branch's
/// tree as a single commit on top of the merge base produces exactly the patch a squash merge
/// would have produced, and `git cherry` then recognises it by patch id.
///
/// The synthetic commit is written into a scratch object directory so the probe leaves the
/// repository's object database untouched.
fn is_squash_merged(repository: &Path, head: &str, base: &str) -> bool {
    let Some(merge_base) = git_stdout(repository, &["merge-base", base, head]) else {
        return false;
    };
    let Some(tree) = git_stdout(repository, &["rev-parse", &format!("{head}^{{tree}}")]) else {
        return false;
    };
    let Some(objects) = git_stdout(
        repository,
        &["rev-parse", "--path-format=absolute", "--git-path", "objects"],
    ) else {
        return false;
    };
    let Some(scratch) = ScratchObjectDir::new() else {
        return false;
    };

    let mut probe = Command::new("git");
    probe
        .arg("-C")
        .arg(repository)
        .env("GIT_OBJECT_DIRECTORY", &scratch.path)
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects)
        .env("GIT_AUTHOR_NAME", "node-modules-cleaner")
        .env("GIT_AUTHOR_EMAIL", "probe@node-modules-cleaner.invalid")
        .env("GIT_COMMITTER_NAME", "node-modules-cleaner")
        .env("GIT_COMMITTER_EMAIL", "probe@node-modules-cleaner.invalid")
        .args(["commit-tree", &tree, "-p", &merge_base, "-m", "_"]);

    let synthetic = match probe.output() {
        Ok(output) if output.status.success() => {
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }
        _ => return false,
    };
    if synthetic.is_empty() {
        return false;
    }

    let cherry = Command::new("git")
        .arg("-C")
        .arg(repository)
        .env("GIT_OBJECT_DIRECTORY", &scratch.path)
        .env("GIT_ALTERNATE_OBJECT_DIRECTORIES", &objects)
        .args(["cherry", base, &synthetic])
        .output();

    match cherry {
        Ok(output) if output.status.success() => String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .is_some_and(|line| line.starts_with('-')),
        _ => false,
    }
}

fn is_junk_entry(entry: &str) -> bool {
    let trimmed = entry.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or(trimmed);
    if name.is_empty() {
        return false;
    }
    JUNK_EXACT_NAMES.contains(&name)
        || JUNK_NAME_PREFIXES
            .iter()
            .any(|prefix| name.starts_with(prefix))
}

/// Split `git status --porcelain -z` into real changes, ignored content, and OS/watcher junk.
///
/// Junk is the reason this is not a two-way split: a stray `.watchman-cookie-*` is enough to
/// make `git worktree remove` refuse, but it is not work anybody wants protected.
fn classify_status_output(stdout: &[u8]) -> WorktreeStatus {
    let mut status = WorktreeStatus::default();
    let fields: Vec<&[u8]> = stdout.split(|byte| *byte == 0).collect();
    let mut index = 0;

    while index < fields.len() {
        let field = fields[index];
        index += 1;
        if field.len() < 4 {
            continue;
        }

        let code = &field[0..2];
        // Rename and copy entries carry the original path in a second field; skipping it keeps
        // that path from being read back as an entry of its own.
        if code.contains(&b'R') || code.contains(&b'C') {
            index += 1;
        }

        if code == b"!!" {
            status.has_ignored_files = true;
        } else if code == b"??" && is_junk_entry(&String::from_utf8_lossy(&field[3..])) {
            status.junk_file_count += 1;
        } else {
            status.is_dirty = true;
        }
    }

    status
}

fn worktree_status(worktree: &Path) -> WorktreeStatus {
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree)
        .args([
            "status",
            "--porcelain",
            "-z",
            "--ignored=matching",
            "--untracked-files=normal",
        ])
        .output();

    match output {
        Ok(output) if output.status.success() => classify_status_output(&output.stdout),
        // Fail closed: an unreadable worktree is never offered for removal.
        _ => WorktreeStatus {
            is_dirty: true,
            has_ignored_files: false,
            junk_file_count: 0,
        },
    }
}

fn git_common_dir(directory: &Path) -> Option<PathBuf> {
    git_stdout(
        directory,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .map(PathBuf::from)
}

/// `git fetch`, but it can never sit forever on a credential prompt and can never outlive the
/// timeout. Both matter because the scan fetches every discovered repository.
fn fetch_repository(repository: &Path) -> Result<(), String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["fetch", "--all", "--quiet", "--no-tags"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_SSH_COMMAND", "ssh -oBatchMode=yes")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to run git fetch: {error}"))?;

    let deadline = Instant::now() + FETCH_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return Ok(());
                }
                let mut detail = String::new();
                if let Some(stderr) = child.stderr.as_mut() {
                    let _ = stderr.read_to_string(&mut detail);
                }
                // First meaningful line only: a dead remote produces a four-line block that
                // would otherwise be pasted verbatim into the UI's error bar on every scan.
                let detail = detail
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or_default()
                    .to_string();
                return Err(if detail.is_empty() {
                    "git fetch failed".to_string()
                } else {
                    detail
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!(
                        "git fetch timed out after {}s",
                        FETCH_TIMEOUT.as_secs()
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => return Err(format!("Failed to wait for git fetch: {error}")),
        }
    }
}

fn collect_merged_worktrees(repository: &Path) -> Result<Vec<MergedWorktree>, String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
        .map_err(|error| format!("Failed to run git: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }

    let records = parse_worktree_porcelain(&output.stdout);
    let repository_path = records
        .first()
        .map(|record| record.path.clone())
        .ok_or_else(|| "Git did not return a primary worktree".to_string())?;

    // No linked worktrees, nothing to clean. Checked before the base branches so a repository
    // without commits yet — which has no branches to compare against — stays quiet instead of
    // being reported as unscannable.
    if records.len() <= 1 {
        return Ok(Vec::new());
    }

    let bases = find_base_branches(repository);
    if bases.branches.is_empty() {
        return Err("Could not determine the repository's default branch".to_string());
    }

    let repository_name = repository_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Repository")
        .to_string();
    let mut candidates = Vec::new();

    for record in records.into_iter().skip(1) {
        if record.is_bare {
            continue;
        }

        let is_detached = record.is_detached || record.branch.is_none();
        let branch_label = record
            .branch
            .clone()
            .unwrap_or_else(|| DETACHED_BRANCH_LABEL.to_string());

        // A worktree whose directory is gone leaves a registration behind. It is not dirty and
        // it is not merged — it is administrative litter, and `git worktree prune` is the fix.
        if record.prunable_reason.is_some() || !record.path.exists() {
            candidates.push(MergedWorktree {
                path: record.path.to_string_lossy().to_string(),
                branch: branch_label,
                repository_path: repository_path.to_string_lossy().to_string(),
                repository_name: repository_name.clone(),
                base_branch: String::new(),
                size: 0,
                is_dirty: false,
                has_ignored_files: false,
                is_locked: record.lock_reason.is_some(),
                lock_reason: record.lock_reason.filter(|reason| !reason.is_empty()),
                head: record.head.clone(),
                state: STATE_STALE.to_string(),
                is_detached,
                junk_file_count: 0,
                stale_reason: record
                    .prunable_reason
                    .filter(|reason| !reason.is_empty())
                    .or_else(|| Some("worktree directory is missing".to_string())),
            });
            continue;
        }

        if record.head.is_empty() {
            continue;
        }

        // Never offer the worktree that holds a base branch itself. Its HEAD is trivially an
        // ancestor of the base, which would otherwise read as "merged, safe to delete".
        if let Some(branch) = record.branch.as_deref() {
            if bases
                .branches
                .iter()
                .any(|base| base_branch_local_name(base) == branch)
            {
                continue;
            }
        }

        let mut matched: Option<(String, &str)> = None;
        for base in &bases.branches {
            if is_ancestor(repository, &record.head, base)? {
                matched = Some((base.clone(), STATE_MERGED));
                break;
            }
            if is_squash_merged(repository, &record.head, base) {
                matched = Some((base.clone(), STATE_SQUASHED));
                break;
            }
        }
        let Some((base_branch, state)) = matched else {
            continue;
        };

        let status = worktree_status(&record.path);

        candidates.push(MergedWorktree {
            path: record.path.to_string_lossy().to_string(),
            branch: branch_label,
            repository_path: repository_path.to_string_lossy().to_string(),
            repository_name: repository_name.clone(),
            base_branch,
            size: 0,
            is_dirty: status.is_dirty,
            has_ignored_files: status.has_ignored_files,
            is_locked: record.lock_reason.is_some(),
            lock_reason: record.lock_reason.filter(|reason| !reason.is_empty()),
            head: record.head.clone(),
            state: state.to_string(),
            is_detached,
            junk_file_count: status.junk_file_count,
            stale_reason: None,
        });
    }

    Ok(candidates)
}

fn registered_worktree_paths(repository: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "list", "--porcelain", "-z"])
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    parse_worktree_porcelain(&output.stdout)
        .into_iter()
        .map(|record| record.path)
        .collect()
}

/// Match a requested path against a scan result. Compares literally first so a stale entry —
/// whose directory no longer exists and therefore cannot be canonicalized — still matches.
fn find_candidate<'a>(candidates: &'a [MergedWorktree], worktree: &Path) -> Option<&'a MergedWorktree> {
    let canonical = worktree.canonicalize().ok();
    candidates.iter().find(|candidate| {
        let candidate_path = Path::new(&candidate.path);
        candidate_path == worktree
            || canonical
                .as_ref()
                .is_some_and(|path| candidate_path.canonicalize().ok().as_ref() == Some(path))
    })
}

fn is_registered(repository: &Path, worktree: &Path) -> bool {
    let canonical = worktree.canonicalize().ok();
    registered_worktree_paths(repository).into_iter().any(|path| {
        path == worktree
            || canonical
                .as_ref()
                .is_some_and(|target| path.canonicalize().ok().as_ref() == Some(target))
    })
}

fn failed_removal(path: String, error: impl Into<String>) -> WorktreeDeleteResult {
    WorktreeDeleteResult {
        success: false,
        path,
        error: Some(error.into()),
    }
}

/// Exactly one `-f`, never two. A second `-f` is what lets git remove a *locked* worktree, so
/// keeping it at one means the junk retry below can never bypass a lock — including one taken
/// between the lock check and this call.
fn run_worktree_remove(repository: &Path, worktree: &Path, force: bool) -> Result<(), String> {
    let mut command = Command::new("git");
    command.arg("-C").arg(repository).args(["worktree", "remove"]);
    if force {
        command.arg("-f");
    }
    command.arg("--").arg(worktree);

    match command.output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        Err(error) => Err(format!("Failed to run git: {error}")),
    }
}

/// Remove one worktree against an already-collected view of its repository.
///
/// The scan is a snapshot, not the truth: another session can commit into a worktree or remove
/// it entirely between the scan and this call, so everything that decides safety is re-read
/// here and each outcome is reported distinctly.
fn remove_prepared_worktree(
    repository: &Path,
    worktree: &Path,
    expected_head: Option<&str>,
    candidates: &[MergedWorktree],
) -> WorktreeDeleteResult {
    let path = worktree.to_string_lossy().to_string();

    // Checked before anything else: a worktree someone has committed into since the scan is no
    // longer merged either, and "someone committed here" is the message that explains why.
    if let Some(expected) = expected_head.filter(|head| !head.is_empty()) {
        if let Some(current) = git_stdout(worktree, &["rev-parse", "HEAD"]) {
            if current != expected {
                return failed_removal(path, "Changed since scan — someone committed here");
            }
        }
    }

    let Some(candidate) = find_candidate(candidates, worktree) else {
        return failed_removal(
            path,
            if is_registered(repository, worktree) {
                "No longer merged into the repository's base branches"
            } else {
                "No longer registered"
            },
        );
    };

    if candidate.state == STATE_STALE {
        return prune_stale_worktree(repository, worktree);
    }

    let Ok(requested_path) = worktree.canonicalize() else {
        return failed_removal(path, "Already removed (by another process)");
    };

    if candidate.is_locked {
        let detail = candidate
            .lock_reason
            .as_ref()
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default();
        return failed_removal(path, format!("Worktree is locked{detail}"));
    }

    let status = worktree_status(worktree);
    if status.is_dirty {
        return failed_removal(path, "Worktree has uncommitted changes");
    }

    match run_worktree_remove(repository, &requested_path, false) {
        Ok(()) => WorktreeDeleteResult {
            success: true,
            path,
            error: None,
        },
        Err(error) => {
            // Untracked junk makes git refuse, and a watcher cookie can appear between the
            // status read above and the removal. Re-read rather than trusting either snapshot:
            // force only when nothing but ignored content and junk is actually present.
            let fresh = worktree_status(worktree);
            if fresh.is_dirty {
                return failed_removal(path, error);
            }
            match run_worktree_remove(repository, &requested_path, true) {
                Ok(()) => WorktreeDeleteResult {
                    success: true,
                    path,
                    error: None,
                },
                Err(force_error) => failed_removal(path, force_error),
            }
        }
    }
}

/// Clear a registration whose directory is gone. `git worktree prune` only touches metadata in
/// `.git/worktrees`, so it cannot destroy anything on disk.
fn prune_stale_worktree(repository: &Path, worktree: &Path) -> WorktreeDeleteResult {
    let path = worktree.to_string_lossy().to_string();
    let _ = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "prune"])
        .output();

    if is_registered(repository, worktree) {
        failed_removal(path, "Git did not prune this stale worktree entry")
    } else {
        WorktreeDeleteResult {
            success: true,
            path,
            error: None,
        }
    }
}

/// Single-worktree removal that collects the repository view itself. The batch command shares
/// one collection across a repository instead, so this exists only for tests that exercise one
/// removal in isolation.
#[cfg(test)]
fn remove_merged_worktree(repository: &Path, worktree: &Path) -> WorktreeDeleteResult {
    match collect_merged_worktrees(repository) {
        Ok(candidates) => remove_prepared_worktree(repository, worktree, None, &candidates),
        Err(error) => failed_removal(worktree.to_string_lossy().to_string(), error),
    }
}

fn discover_git_repositories(scan_path: &Path) -> Vec<PathBuf> {
    let mut repositories = Vec::new();
    let mut pending = vec![scan_path.to_path_buf()];

    while let Some(directory) = pending.pop() {
        if directory.join(".git").exists() {
            repositories.push(directory);
            continue;
        }

        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.filter_map(Result::ok) {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }

            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name.starts_with('.') || name == "node_modules" {
                continue;
            }
            pending.push(entry.path());
        }
    }

    repositories
}

/// Collapse discovered directories to one entry per real repository.
///
/// A linked worktree has a `.git` *file*, so the walk classifies each of them as a repository
/// of its own — a directory of twenty worktrees looks like twenty repositories, each returning
/// the same worktree list. Resolving the common git dir collapses them before any of the
/// expensive per-repository work (fetch, squash probes) runs even once.
fn unique_repositories(discovered: Vec<PathBuf>) -> Vec<(PathBuf, bool)> {
    let mut unique = Vec::new();
    let mut seen = HashSet::new();

    for directory in discovered {
        match git_common_dir(&directory) {
            Some(common) => {
                if !seen.insert(common.clone()) {
                    continue;
                }
                // Drive the repository from its main worktree rather than from whichever
                // linked worktree the directory walk happened to reach first, so the result
                // does not depend on traversal order.
                let main_worktree = common
                    .file_name()
                    .is_some_and(|name| name == ".git")
                    .then(|| common.parent().map(Path::to_path_buf))
                    .flatten()
                    .filter(|path| path.is_dir());
                unique.push((main_worktree.unwrap_or(directory), true));
            }
            // Not resolvable as a repository. Keep it so it still counts towards the
            // "could not scan anything" check, but never fetch it.
            None => unique.push((directory, false)),
        }
    }

    unique
}

fn scan_for_merged_worktrees_in(scan_path: &Path) -> Result<WorktreeScanResult, String> {
    if !scan_path.exists() {
        return Err("Path does not exist".to_string());
    }
    if !scan_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let repositories = unique_repositories(discover_git_repositories(scan_path));
    let repository_count = repositories.len();

    // Remote-tracking refs decide what counts as merged, so they are refreshed before anything
    // is compared. A repository that cannot be fetched is still scanned against what is local.
    let mut warnings: Vec<String> = repositories
        .par_iter()
        .filter(|(_, resolvable)| *resolvable)
        .filter_map(|(repository, _)| {
            fetch_repository(repository)
                .err()
                .map(|error| format!("{}: could not fetch, results may be stale ({error})", repository.display()))
        })
        .collect();

    let mut diagnostics = Vec::new();
    let mut evaluated_repositories = 0;
    let mut repository_errors = Vec::new();
    let mut seen = HashSet::new();
    let mut worktrees = Vec::new();

    for (repository, resolvable) in repositories {
        if resolvable {
            let bases = find_base_branches(&repository);
            if !bases.branches.is_empty() {
                diagnostics.push(format!(
                    "{}: compared against {}{}",
                    repository.display(),
                    bases.branches.join(", "),
                    if bases.used_local_fallback {
                        " (local branches — no remote base found, these can be out of date)"
                    } else {
                        ""
                    }
                ));
            }
        }

        let candidates = match collect_merged_worktrees(&repository) {
            Ok(candidates) => {
                evaluated_repositories += 1;
                candidates
            }
            Err(error) => {
                repository_errors.push(format!("{}: {error}", repository.display()));
                continue;
            }
        };
        for mut candidate in candidates {
            let identity = (candidate.repository_path.clone(), candidate.path.clone());
            if !seen.insert(identity) {
                continue;
            }
            candidate.size = calculate_dir_size(Path::new(&candidate.path));
            worktrees.push(candidate);
        }
    }

    if repository_count > 0 && evaluated_repositories == 0 {
        let detail = repository_errors
            .first()
            .map(|error| format!(" {error}"))
            .unwrap_or_default();
        return Err(format!(
            "Could not scan any of {repository_count} Git repositories.{detail}"
        ));
    }

    worktrees.sort_by_key(|worktree| Reverse(worktree.size));
    let total_size = worktrees
        .iter()
        .filter(|worktree| !worktree.is_dirty && !worktree.is_locked)
        .map(|worktree| worktree.size)
        .sum();

    warnings.extend(repository_errors);

    Ok(WorktreeScanResult {
        worktrees,
        total_size,
        scan_path: scan_path.to_string_lossy().to_string(),
        warnings,
        diagnostics,
    })
}

/// How many worktrees under `root` are ready to be removed right now.
///
/// This is the number the menu bar shows, and it deliberately goes through the same
/// `collect_merged_worktrees` the window uses, so the two can never disagree. It skips the two
/// expensive steps of a full scan: no `fetch_repository` (a background tick must not touch the
/// network) and no `calculate_dir_size` (walking `node_modules` is by far the slowest part and a
/// count does not need bytes).
pub(crate) fn count_removable_worktrees(root: &Path) -> usize {
    if !root.is_dir() {
        return 0;
    }

    let mut seen = HashSet::new();
    let mut removable = 0;

    for (repository, resolvable) in unique_repositories(discover_git_repositories(root)) {
        if !resolvable {
            continue;
        }
        let Ok(candidates) = collect_merged_worktrees(&repository) else {
            continue;
        };
        for candidate in candidates {
            if !seen.insert((candidate.repository_path.clone(), candidate.path.clone())) {
                continue;
            }
            if !candidate.is_dirty && !candidate.is_locked && candidate.state != STATE_STALE {
                removable += 1;
            }
        }
    }

    removable
}

#[tauri::command]
pub async fn scan_for_merged_worktrees(path: String) -> Result<WorktreeScanResult, String> {
    scan_for_merged_worktrees_in(Path::new(&path))
}

#[tauri::command]
pub async fn delete_merged_worktrees(removals: Vec<WorktreeRemoval>) -> Vec<WorktreeDeleteResult> {
    delete_merged_worktrees_in(removals)
}

/// Removals are grouped by repository so each repository is inspected once instead of once per
/// worktree, and so the stale entries in it can be cleared with a single `git worktree prune`.
/// Within a repository the removals stay sequential — they all contend for the same
/// `.git/worktrees` administrative files.
fn delete_merged_worktrees_in(removals: Vec<WorktreeRemoval>) -> Vec<WorktreeDeleteResult> {
    let mut results: Vec<Option<WorktreeDeleteResult>> = (0..removals.len()).map(|_| None).collect();
    let mut order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<usize>> = HashMap::new();

    for (index, removal) in removals.iter().enumerate() {
        groups
            .entry(removal.repository_path.clone())
            .or_insert_with(|| {
                order.push(removal.repository_path.clone());
                Vec::new()
            })
            .push(index);
    }

    for repository_path in order {
        let repository = PathBuf::from(&repository_path);
        let indices = groups.remove(&repository_path).unwrap_or_default();

        let candidates = match collect_merged_worktrees(&repository) {
            Ok(candidates) => candidates,
            Err(error) => {
                for index in indices {
                    results[index] = Some(failed_removal(
                        removals[index].worktree_path.clone(),
                        error.clone(),
                    ));
                }
                continue;
            }
        };

        let mut stale_indices = Vec::new();
        let mut removed_any = false;

        for index in indices {
            let removal = &removals[index];
            let worktree = Path::new(&removal.worktree_path);
            if find_candidate(&candidates, worktree)
                .is_some_and(|candidate| candidate.state == STATE_STALE)
            {
                stale_indices.push(index);
                continue;
            }

            let result = remove_prepared_worktree(
                &repository,
                worktree,
                Some(&removal.head),
                &candidates,
            );
            removed_any |= result.success;
            results[index] = Some(result);
        }

        if removed_any || !stale_indices.is_empty() {
            let _ = Command::new("git")
                .arg("-C")
                .arg(&repository)
                .args(["worktree", "prune"])
                .output();
        }

        for index in stale_indices {
            let worktree = Path::new(&removals[index].worktree_path);
            let pruned = !is_registered(&repository, worktree);
            results[index] = Some(if pruned {
                WorktreeDeleteResult {
                    success: true,
                    path: removals[index].worktree_path.clone(),
                    error: None,
                }
            } else {
                failed_removal(
                    removals[index].worktree_path.clone(),
                    "Git did not prune this stale worktree entry",
                )
            });
        }
    }

    results
        .into_iter()
        .enumerate()
        .map(|(index, result)| {
            result.unwrap_or_else(|| {
                failed_removal(
                    removals[index].worktree_path.clone(),
                    "Worktree was not processed",
                )
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        classify_status_output, collect_merged_worktrees,
        count_removable_worktrees, delete_merged_worktrees_in, find_base_branches, is_ancestor,
        is_junk_entry,
        is_squash_merged, parse_worktree_porcelain, remove_merged_worktree,
        scan_for_merged_worktrees_in, WorktreeRemoval,
    };
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_REPO_ID: AtomicU64 = AtomicU64::new(0);

    struct TestRepo {
        root: PathBuf,
    }

    impl TestRepo {
        fn new() -> Self {
            let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "node-modules-cleaner-git-test-{}-{id}",
                std::process::id()
            ));
            Self::new_at(root)
        }

        fn new_at(root: PathBuf) -> Self {
            fs::create_dir_all(&root).expect("create temporary repository");

            let repo = Self { root };
            repo.git(&["init", "-b", "main"]);
            fs::write(repo.root.join("README.md"), "initial\n").expect("write fixture");
            repo.git(&["add", "README.md"]);
            repo.git(&[
                "-c",
                "user.name=Node Modules Cleaner Tests",
                "-c",
                "user.email=tests@example.com",
                "commit",
                "-m",
                "initial",
            ]);
            repo
        }

        fn git(&self, args: &[&str]) -> String {
            let output = Command::new("git")
                .arg("-C")
                .arg(&self.root)
                .args(args)
                .output()
                .expect("run git fixture command");
            assert!(
                output.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&output.stderr)
            );
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        }

        fn path(&self) -> &Path {
            &self.root
        }

        fn add_feature_worktree(&self, branch: &str) -> PathBuf {
            self.git(&["branch", branch]);
            let worktree_path = self.root.join(branch.replace('/', "-"));
            self.git(&[
                "worktree",
                "add",
                worktree_path.to_str().expect("UTF-8 fixture path"),
                branch,
            ]);
            worktree_path
        }

        fn commit_all(&self, message: &str) {
            self.git(&[
                "-c",
                "user.name=Node Modules Cleaner Tests",
                "-c",
                "user.email=tests@example.com",
                "commit",
                "-m",
                message,
            ]);
        }

        fn head(&self) -> String {
            self.git(&["rev-parse", "HEAD"])
        }

        fn add_detached_worktree(&self, name: &str, commit: &str) -> PathBuf {
            let worktree_path = self.root.join(name);
            self.git(&[
                "worktree",
                "add",
                "--detach",
                worktree_path.to_str().expect("UTF-8 fixture path"),
                commit,
            ]);
            worktree_path
        }

        fn commit_file(&self, worktree: &Path, file_name: &str) {
            fs::write(worktree.join(file_name), "feature\n").expect("write feature fixture");
            let output = Command::new("git")
                .arg("-C")
                .arg(worktree)
                .args(["add", file_name])
                .output()
                .expect("stage fixture file");
            assert!(output.status.success());

            let output = Command::new("git")
                .arg("-C")
                .arg(worktree)
                .args([
                    "-c",
                    "user.name=Node Modules Cleaner Tests",
                    "-c",
                    "user.email=tests@example.com",
                    "commit",
                    "-m",
                    "feature",
                ])
                .output()
                .expect("commit fixture file");
            assert!(
                output.status.success(),
                "git commit failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }

    impl Drop for TestRepo {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn parses_porcelain_records_without_splitting_paths_on_spaces() {
        let output = b"worktree /projects/main repo\0HEAD aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\0branch refs/heads/main\0\0worktree /projects/topic tree\0HEAD bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\0branch refs/heads/feature/done\0\0";

        let records = parse_worktree_porcelain(output);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].path, PathBuf::from("/projects/main repo"));
        assert_eq!(records[0].branch.as_deref(), Some("main"));
        assert_eq!(records[1].path, PathBuf::from("/projects/topic tree"));
        assert_eq!(records[1].head, "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert_eq!(records[1].branch.as_deref(), Some("feature/done"));
        assert!(!records[1].is_detached);
    }

    #[test]
    fn falls_back_to_local_main_when_origin_head_is_missing() {
        let repo = TestRepo::new();

        let bases = find_base_branches(repo.path());

        assert_eq!(bases.branches, vec!["main".to_string()]);
        assert!(bases.used_local_fallback);
    }

    #[test]
    fn prefers_origin_head_over_local_default_branches() {
        let repo = TestRepo::new();
        repo.git(&["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ]);

        let bases = find_base_branches(repo.path());

        assert_eq!(bases.branches, vec!["origin/trunk".to_string()]);
        assert!(!bases.used_local_fallback);
    }

    #[test]
    fn falls_back_to_local_main_when_origin_head_is_dangling() {
        let repo = TestRepo::new();
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/missing",
        ]);

        let bases = find_base_branches(repo.path());

        assert_eq!(bases.branches, vec!["main".to_string()]);
        assert!(bases.used_local_fallback);
    }

    #[test]
    fn excludes_worktree_whose_head_is_not_merged_into_default_branch() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/open");
        repo.commit_file(&worktree, "open.txt");

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert!(candidates.is_empty());
    }

    #[test]
    fn returns_merged_worktree_and_marks_uncommitted_changes_as_dirty() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/done");
        repo.commit_file(&worktree, "done.txt");
        repo.git(&["merge", "--ff-only", "feature/done"]);

        let clean_candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(clean_candidates.len(), 1);
        assert_eq!(
            Path::new(&clean_candidates[0].path),
            worktree.canonicalize().expect("canonical worktree path")
        );
        assert_eq!(clean_candidates[0].branch, "feature/done");
        assert_eq!(clean_candidates[0].base_branch, "main");
        assert!(!clean_candidates[0].is_dirty);

        fs::write(worktree.join("uncommitted.txt"), "keep me\n")
            .expect("write uncommitted fixture");
        let dirty_candidates = collect_merged_worktrees(repo.path()).expect("rescan worktrees");

        assert_eq!(dirty_candidates.len(), 1);
        assert!(dirty_candidates[0].is_dirty);
    }

    #[test]
    fn removes_clean_merged_worktree_without_deleting_its_branch() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/remove-me");
        repo.commit_file(&worktree, "done.txt");
        repo.git(&["merge", "--ff-only", "feature/remove-me"]);
        let canonical_worktree = worktree.canonicalize().expect("canonical worktree path");

        let result = remove_merged_worktree(repo.path(), &canonical_worktree);

        assert!(result.success, "{}", result.error.unwrap_or_default());
        assert!(!canonical_worktree.exists());
        repo.git(&[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/feature/remove-me",
        ]);
    }

    #[test]
    fn refuses_to_remove_merged_worktree_with_uncommitted_changes() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/keep-dirty");
        repo.git(&["merge", "--ff-only", "feature/keep-dirty"]);
        fs::write(worktree.join("uncommitted.txt"), "keep me\n")
            .expect("write uncommitted fixture");
        let canonical_worktree = worktree.canonicalize().expect("canonical worktree path");

        let result = remove_merged_worktree(repo.path(), &canonical_worktree);

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("uncommitted changes")));
        assert!(canonical_worktree.exists());
    }

    #[test]
    fn scans_nested_repository_and_totals_only_removable_worktrees() {
        let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let scan_root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-scan-test-{}-{id}",
            std::process::id()
        ));
        let repo = TestRepo::new_at(scan_root.join("group").join("sample-repo"));
        let clean_worktree = repo.add_feature_worktree("feature/clean");
        repo.git(&["merge", "--ff-only", "feature/clean"]);
        let dirty_worktree = repo.add_feature_worktree("feature/dirty");
        repo.git(&["merge", "--ff-only", "feature/dirty"]);
        fs::write(dirty_worktree.join("uncommitted.txt"), "keep me\n")
            .expect("write uncommitted fixture");

        let result = scan_for_merged_worktrees_in(&scan_root).expect("scan selected path");

        assert_eq!(result.worktrees.len(), 2);
        assert!(result
            .worktrees
            .iter()
            .all(|worktree| worktree.repository_name == "sample-repo"));
        assert!(result.worktrees.iter().all(|worktree| {
            worktree.repository_path
                == repo
                    .path()
                    .canonicalize()
                    .expect("canonical repository path")
        }));
        let clean = result
            .worktrees
            .iter()
            .find(|worktree| worktree.branch == "feature/clean")
            .expect("clean worktree");
        let dirty = result
            .worktrees
            .iter()
            .find(|worktree| worktree.branch == "feature/dirty")
            .expect("dirty worktree");
        assert!(!clean.is_dirty);
        assert!(dirty.is_dirty);
        assert_eq!(result.total_size, clean.size);
        assert_eq!(
            Path::new(&clean.path),
            clean_worktree
                .canonicalize()
                .expect("canonical worktree path")
        );

        drop(repo);
        let _ = fs::remove_dir_all(scan_root);
    }

    #[test]
    fn returns_error_when_discovered_repositories_cannot_be_evaluated() {
        let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let scan_root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-invalid-scan-test-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(scan_root.join("broken-repo").join(".git"))
            .expect("create invalid repository fixture");

        let error = scan_for_merged_worktrees_in(&scan_root)
            .expect_err("invalid repository should not look like an empty successful scan");

        assert!(error.contains("Could not scan any of 1 Git repositories"));
        let _ = fs::remove_dir_all(scan_root);
    }

    #[test]
    fn returns_warnings_for_failed_repositories_when_others_are_scanned() {
        let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let scan_root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-mixed-scan-test-{}-{id}",
            std::process::id()
        ));
        let repo = TestRepo::new_at(scan_root.join("valid-repo"));
        fs::create_dir_all(scan_root.join("broken-repo").join(".git"))
            .expect("create invalid repository fixture");

        let result = scan_for_merged_worktrees_in(&scan_root).expect("scan valid repository");

        assert_eq!(result.warnings.len(), 1);
        assert!(result.warnings[0].contains("broken-repo"));

        drop(repo);
        let _ = fs::remove_dir_all(scan_root);
    }

    #[test]
    fn distinguishes_merge_base_errors_from_not_merged_results() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/open-for-check");
        repo.commit_file(&worktree, "open.txt");

        assert!(!is_ancestor(repo.path(), "feature/open-for-check", "main")
            .expect("valid ancestry check"));
        let error = is_ancestor(repo.path(), "missing-commit", "main")
            .expect_err("invalid commit should be a Git error");

        assert!(error.contains("git merge-base failed"));
    }

    #[test]
    fn reports_ignored_content_without_treating_it_as_dirty() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/ignored-content");
        repo.commit_file(&worktree, ".gitignore");
        repo.git(&["merge", "--ff-only", "feature/ignored-content"]);
        fs::write(worktree.join("feature"), "local ignored content\n")
            .expect("write ignored fixture");

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].is_dirty);
        assert!(candidates[0].has_ignored_files);
    }

    #[test]
    fn reports_and_refuses_to_remove_locked_worktree() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/locked");
        repo.git(&[
            "worktree",
            "lock",
            "--reason",
            "in use by another tool",
            worktree.to_str().expect("UTF-8 fixture path"),
        ]);

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].is_locked);
        assert_eq!(
            candidates[0].lock_reason.as_deref(),
            Some("in use by another tool")
        );

        let canonical_worktree = worktree.canonicalize().expect("canonical worktree path");
        let result = remove_merged_worktree(repo.path(), &canonical_worktree);

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("locked")));
        assert!(canonical_worktree.exists());
    }

    #[test]
    fn finds_worktree_merged_only_into_origin_development() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/dev-only");
        repo.commit_file(&worktree, "dev.txt");
        let feature_head = repo.git(&["rev-parse", "feature/dev-only"]);

        // origin/HEAD points at main, which never received this branch; the work landed on
        // origin/development instead. Comparing against origin/HEAD alone misses it entirely.
        repo.git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ]);
        repo.git(&["update-ref", "refs/remotes/origin/development", &feature_head]);

        let bases = find_base_branches(repo.path());
        assert_eq!(
            bases.branches,
            vec![
                "origin/main".to_string(),
                "origin/development".to_string()
            ]
        );

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].branch, "feature/dev-only");
        assert_eq!(candidates[0].base_branch, "origin/development");
        assert_eq!(candidates[0].state, "merged");
    }

    #[test]
    fn detects_squash_merged_branch() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/squashed");
        repo.commit_file(&worktree, "first.txt");
        repo.commit_file(&worktree, "second.txt");
        repo.git(&["merge", "--squash", "feature/squashed"]);
        repo.commit_all("squashed feature");

        // A squash merge never makes the branch an ancestor of the base.
        assert!(!is_ancestor(repo.path(), "feature/squashed", "main").expect("ancestry check"));

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].branch, "feature/squashed");
        assert_eq!(candidates[0].state, "squashed");
        assert!(!candidates[0].is_dirty);
    }

    #[test]
    fn squash_probe_leaves_the_repository_object_database_untouched() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/probe");
        repo.commit_file(&worktree, "probe.txt");
        repo.git(&["merge", "--squash", "feature/probe"]);
        repo.commit_all("squashed probe");

        let loose_before = repo.git(&["count-objects", "-v"]);

        assert!(is_squash_merged(
            repo.path(),
            &repo.git(&["rev-parse", "feature/probe"]),
            "main"
        ));

        assert_eq!(repo.git(&["count-objects", "-v"]), loose_before);
    }

    #[test]
    fn does_not_treat_unrelated_branch_content_as_squash_merged() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/never-merged");
        repo.commit_file(&worktree, "open.txt");

        assert!(!is_squash_merged(
            repo.path(),
            &repo.git(&["rev-parse", "feature/never-merged"]),
            "main"
        ));
    }

    #[test]
    fn treats_operating_system_and_watcher_junk_as_removable_noise() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/junk");
        repo.git(&["merge", "--ff-only", "feature/junk"]);
        fs::write(worktree.join(".DS_Store"), "finder
").expect("write junk fixture");
        fs::write(worktree.join(".watchman-cookie-host-1"), "").expect("write junk fixture");

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].is_dirty);
        assert_eq!(candidates[0].junk_file_count, 2);
    }

    #[test]
    fn treats_other_untracked_files_as_real_changes() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/untracked-work");
        repo.git(&["merge", "--ff-only", "feature/untracked-work"]);
        fs::create_dir_all(worktree.join(".playwright-mcp")).expect("create fixture directory");
        fs::write(worktree.join(".playwright-mcp").join("trace.log"), "log
")
            .expect("write untracked fixture");

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].is_dirty);
        assert_eq!(candidates[0].junk_file_count, 0);
    }

    #[test]
    fn matches_junk_by_file_name_not_by_path() {
        assert!(is_junk_entry(".DS_Store"));
        assert!(is_junk_entry("apps/web/.DS_Store"));
        assert!(is_junk_entry(".watchman-cookie-host-12345"));
        assert!(!is_junk_entry(".playwright-mcp/"));
        assert!(!is_junk_entry("src/DS_Store.ts"));
    }

    #[test]
    fn skips_the_original_path_of_a_rename_entry() {
        // Without consuming the second field, the original path is parsed as an entry of its
        // own — here it is shaped like an untracked junk file to make that visible.
        let status = classify_status_output(b"R  renamed.txt\0?? .DS_Store\0");

        assert!(status.is_dirty);
        assert_eq!(status.junk_file_count, 0);
    }

    #[test]
    fn includes_merged_worktree_with_a_detached_head() {
        let repo = TestRepo::new();
        let worktree = repo.add_detached_worktree("detached-review", &repo.head());

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].branch, "(detached)");
        assert!(candidates[0].is_detached);
        assert!(!candidates[0].is_dirty);
        assert_eq!(
            Path::new(&candidates[0].path),
            worktree.canonicalize().expect("canonical worktree path")
        );
    }

    #[test]
    fn never_offers_the_worktree_that_holds_a_base_branch() {
        let repo = TestRepo::new();
        repo.git(&["update-ref", "refs/remotes/origin/trunk", "HEAD"]);
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ]);
        repo.add_feature_worktree("trunk");

        // Its HEAD trivially is an ancestor of the base, so only the branch-name guard keeps it
        // off the list.
        assert!(is_ancestor(repo.path(), "trunk", "origin/trunk").expect("ancestry check"));

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");

        assert!(candidates.is_empty());
    }

    #[test]
    fn reports_a_missing_worktree_directory_as_stale_and_prunes_it() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/stale");
        repo.git(&["merge", "--ff-only", "feature/stale"]);
        fs::remove_dir_all(&worktree).expect("simulate a hand-deleted worktree directory");

        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].state, "stale");
        assert!(!candidates[0].is_dirty, "a missing directory is not dirty work");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: candidates[0].repository_path.clone(),
            worktree_path: candidates[0].path.clone(),
            head: candidates[0].head.clone(),
        }]);

        assert_eq!(results.len(), 1);
        assert!(results[0].success, "{}", results[0].error.clone().unwrap_or_default());
        assert!(!repo.git(&["worktree", "list"]).contains("feature/stale"));
    }

    #[test]
    fn removes_a_merged_worktree_that_only_junk_was_blocking() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/junk-blocked");
        repo.git(&["merge", "--ff-only", "feature/junk-blocked"]);
        fs::write(worktree.join(".watchman-cookie-host-1"), "").expect("write junk fixture");
        let canonical_worktree = worktree.canonicalize().expect("canonical worktree path");

        // Plain removal refuses while the untracked cookie is there.
        assert!(super::run_worktree_remove(repo.path(), &canonical_worktree, false).is_err());

        let result = remove_merged_worktree(repo.path(), &canonical_worktree);

        assert!(result.success, "{}", result.error.unwrap_or_default());
        assert!(!canonical_worktree.exists());
        repo.git(&[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/feature/junk-blocked",
        ]);
    }

    #[test]
    fn does_not_force_removal_of_a_locked_worktree_that_only_holds_junk() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/locked-junk");
        repo.git(&["merge", "--ff-only", "feature/locked-junk"]);
        fs::write(worktree.join(".DS_Store"), "finder\n").expect("write junk fixture");
        repo.git(&[
            "worktree",
            "lock",
            "--reason",
            "held by another tool",
            worktree.to_str().expect("UTF-8 fixture path"),
        ]);
        let canonical_worktree = worktree.canonicalize().expect("canonical worktree path");

        let result = remove_merged_worktree(repo.path(), &canonical_worktree);

        assert!(!result.success);
        assert!(result
            .error
            .as_deref()
            .is_some_and(|error| error.contains("locked")));
        assert!(canonical_worktree.exists());
    }

    #[test]
    fn refuses_to_remove_a_worktree_that_changed_since_the_scan() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/moved-on");
        repo.git(&["merge", "--ff-only", "feature/moved-on"]);
        let candidates = collect_merged_worktrees(repo.path()).expect("scan worktrees");
        assert_eq!(candidates.len(), 1);

        // Another session commits into the worktree between the scan and the removal.
        repo.commit_file(&worktree, "new-work.txt");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: candidates[0].repository_path.clone(),
            worktree_path: candidates[0].path.clone(),
            head: candidates[0].head.clone(),
        }]);

        assert!(!results[0].success);
        assert!(results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Changed since scan")));
        assert!(worktree.exists());
    }

    #[test]
    fn reports_an_unregistered_worktree_distinctly_from_an_unmerged_one() {
        let repo = TestRepo::new();
        let missing = repo.path().join("never-existed");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: repo.path().to_string_lossy().to_string(),
            worktree_path: missing.to_string_lossy().to_string(),
            head: String::new(),
        }]);

        assert!(!results[0].success);
        assert_eq!(results[0].error.as_deref(), Some("No longer registered"));
    }

    #[test]
    fn stays_quiet_for_a_repository_that_has_no_linked_worktrees() {
        let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-empty-repo-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("create fixture repository");
        let output = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["init", "-b", "main"])
            .output()
            .expect("init fixture repository");
        assert!(output.status.success());

        // No commits, therefore no branches to compare against — but also nothing to clean,
        // so this must not surface as a scan failure.
        let candidates = collect_merged_worktrees(&root).expect("repository without worktrees");

        assert!(candidates.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_menu_bar_count_includes_only_worktrees_that_can_actually_be_removed() {
        let id = TEST_REPO_ID.fetch_add(1, Ordering::Relaxed);
        let scan_root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-count-test-{}-{id}",
            std::process::id()
        ));
        let repo = TestRepo::new_at(scan_root.join("sample-repo"));

        let removable = repo.add_feature_worktree("feature/removable");
        repo.git(&["merge", "--ff-only", "feature/removable"]);

        let dirty = repo.add_feature_worktree("feature/dirty");
        repo.git(&["merge", "--ff-only", "feature/dirty"]);
        fs::write(dirty.join("work-in-progress.txt"), "keep me\n").expect("write dirty fixture");

        let locked = repo.add_feature_worktree("feature/locked");
        repo.git(&["merge", "--ff-only", "feature/locked"]);
        repo.git(&[
            "worktree",
            "lock",
            locked.to_str().expect("UTF-8 fixture path"),
        ]);

        let stale = repo.add_feature_worktree("feature/stale");
        repo.git(&["merge", "--ff-only", "feature/stale"]);
        fs::remove_dir_all(&stale).expect("simulate a hand-deleted worktree directory");

        let open = repo.add_feature_worktree("feature/open");
        repo.commit_file(&open, "open.txt");

        // Five merged-or-not worktrees, exactly one of which the app would actually remove.
        assert_eq!(count_removable_worktrees(&scan_root), 1);
        assert!(removable.exists());

        drop(repo);
        let _ = fs::remove_dir_all(scan_root);
    }

    #[test]
    fn the_menu_bar_count_is_zero_for_a_folder_that_is_not_there() {
        let missing = std::env::temp_dir().join("node-modules-cleaner-definitely-missing");

        assert_eq!(count_removable_worktrees(&missing), 0);
    }

    /// Print what the scan finds on the machine it runs on, against a real directory tree.
    ///
    /// `WORKTREE_SCAN_PATH=~/Projects cargo test report_real_worktrees -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn report_real_worktrees() {
        let scan_path = std::env::var("WORKTREE_SCAN_PATH").unwrap_or_else(|_| {
            format!("{}/Projects", std::env::var("HOME").unwrap_or_default())
        });
        let result =
            scan_for_merged_worktrees_in(Path::new(&scan_path)).expect("scan should succeed");

        for note in &result.diagnostics {
            eprintln!("base: {note}");
        }
        for warning in &result.warnings {
            eprintln!("warning: {warning}");
        }
        for worktree in &result.worktrees {
            eprintln!(
                "{:>9}  {:<9} dirty={:<5} junk={:<3} locked={:<5} detached={:<5} {} -> {} [{}]",
                worktree.size,
                worktree.state,
                worktree.is_dirty,
                worktree.junk_file_count,
                worktree.is_locked,
                worktree.is_detached,
                worktree.branch,
                worktree.base_branch,
                worktree.path,
            );
        }
        eprintln!("{} worktrees, total {}", result.worktrees.len(), result.total_size);
    }
}
