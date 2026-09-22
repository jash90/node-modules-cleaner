use crate::commands::calculate_dir_size;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
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

/// How many repositories to fetch from at once.
const FETCH_CONCURRENCY: usize = 8;

/// Every `git` this module runs is built here.
///
/// Three things have to hold for each spawn and previously held for none of them. The child
/// starts in macOS background policy, because thread QoS does not survive `posix_spawn` and
/// practically all of this module's CPU is burned inside these children rather than in the
/// caller. `GIT_OPTIONAL_LOCKS=0` stops Git from opportunistically rewriting `.git/index`
/// during what is meant to be a read-only scan — that rewrite is what turned a quarter-hourly
/// background tick into a steady drip of filesystem events. And the spawn is counted, so a
/// tick can report what it actually cost instead of being estimated.
static GIT_SPAWNS: AtomicU64 = AtomicU64::new(0);

/// Total `git` processes prepared since launch. Counted at construction rather than at exit:
/// every command built in this module is also run, and this way the count survives a spawn
/// that fails.
pub(crate) fn git_spawn_count() -> u64 {
    GIT_SPAWNS.load(Ordering::Relaxed)
}

/// A `git` scoped to `directory`, which is how all but a couple of callers want it.
fn git_command(directory: &Path) -> Command {
    let mut command = Command::new("git");
    command.arg("-C").arg(directory);
    prepare_git_command(&mut command);
    command
}

/// How much to deprioritise Git run on behalf of the menu bar. A nice value, deliberately, and
/// not macOS's `PRIO_DARWIN_BG`.
///
/// `PRIO_DARWIN_BG` looked like the obvious choice and is a trap for this workload. It throttles
/// disk I/O as well as CPU, and measured here that costs nothing while the cache is warm but is
/// catastrophic as soon as the work writes: a scan whose squash probes write Git objects went
/// from about twelve seconds to over ten minutes, and the test suite from 0.16s to 79s for a
/// single case. The tick also serves the `Refresh now` menu item, so a tick that never ends is
/// a broken feature and not merely a slow one.
///
/// Plain niceness gives up the CPU to anything the user is doing — which is the actual
/// complaint — and leaves I/O alone.
const BACKGROUND_NICENESS: i32 = 10;

thread_local! {
    /// Whether Git started from this thread should yield CPU to foreground work.
    ///
    /// Only the tray thread sets it. An unattended refresh nobody is waiting on should lose
    /// every race against a scan somebody just clicked, and both go through these same
    /// functions — so the distinction has to come from the caller. A thread-local carries it
    /// without threading a flag through every signature, because the tick is sequential and
    /// stays on the thread that set it.
    static BACKGROUND_GIT: Cell<bool> = const { Cell::new(false) };
}

/// Run `work` with every Git process it starts deprioritised.
pub(crate) fn with_background_git<T>(work: impl FnOnce() -> T) -> T {
    BACKGROUND_GIT.with(|flag| flag.set(true));
    let result = work();
    BACKGROUND_GIT.with(|flag| flag.set(false));
    result
}

fn prepare_git_command(command: &mut Command) {
    GIT_SPAWNS.fetch_add(1, Ordering::Relaxed);
    command.env("GIT_OPTIONAL_LOCKS", "0");

    #[cfg(unix)]
    if BACKGROUND_GIT.with(Cell::get) {
        use std::os::unix::process::CommandExt;
        // SAFETY: `setpriority` is a bare syscall, so it is async-signal-safe and legal in the
        // window between fork and exec, which is all `pre_exec` requires of it. `PRIO_PROCESS`
        // with `0` is the forked child itself, so nothing else on the system is affected.
        unsafe {
            command.pre_exec(|| {
                libc::setpriority(libc::PRIO_PROCESS, 0, BACKGROUND_NICENESS);
                Ok(())
            });
        }
    }
}

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
    /// The user agreed to lose this worktree's uncommitted changes. Sent only for rows that were
    /// already dirty at scan time: a worktree that went dirty afterwards was never shown to anyone
    /// as dirty, so nobody consented to losing that work, and it is still refused.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeDeleteResult {
    pub success: bool,
    pub path: String,
    pub error: Option<String>,
    /// Removed by something else between the scan and this call. The goal is reached, so this
    /// counts as success: other sessions on the same machine routinely clean up after a merge,
    /// and reporting their work as our failure left the row stuck in the list forever.
    #[serde(default)]
    pub already_gone: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BaseBranches {
    branches: Vec<String>,
    /// Commit each entry of `branches` points at, same order, same length.
    ///
    /// Resolving the bases already had to read these, and the verdict cache needs exactly them
    /// to know whether a previous answer still holds — so they are carried out rather than
    /// looked up a second time.
    tips: Vec<String>,
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
    let output = git_command(directory)
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

/// The local name a base branch corresponds to, so a worktree sitting on `main` can be matched
/// against the base `origin/main`.
fn base_branch_local_name(base: &str) -> &str {
    base.strip_prefix("origin/").unwrap_or(base)
}

/// `refs/remotes/origin/main` -> `origin/main`, `refs/heads/main` -> `main`.
///
/// The inverse of the names the base candidates are written in, done by hand so the result does
/// not depend on which other refs the repository happens to contain.
fn shorten_ref(reference: &str) -> Option<String> {
    reference
        .strip_prefix("refs/remotes/")
        .or_else(|| reference.strip_prefix("refs/heads/"))
        .map(str::to_string)
}

/// Every base a repository might be compared against, resolved in one `git` call.
///
/// This used to probe each candidate on its own — a `symbolic-ref`, then a `rev-parse` per
/// remote candidate, then a `show-ref` per local one — which is five to nine processes for a
/// question `for-each-ref` answers in a single line each. It also hands back the tip commits,
/// which the caller needs anyway.
///
/// Reading `%(symref)` is not optional: `refs/remotes/origin/HEAD` is a pointer, and without
/// following it the base would be the pointer's own name, which no worktree ever matches.
///
/// The names are asked for in full and shortened here rather than with `:short`, because
/// `:short` gives the shortest *unambiguous* name and so depends on which other refs happen to
/// exist — measured on this machine it renders `refs/remotes/origin/HEAD` as `origin` and
/// `refs/remotes/origin/master` as `master` when no local `master` is present. Full names are
/// the same in every repository.
///
/// A dangling `origin/HEAD` comes back with an empty `%(objectname)` and is skipped, leaving
/// the local fallback to answer.
fn find_base_branches(repository: &Path) -> BaseBranches {
    let mut patterns: Vec<String> = vec!["refs/remotes/origin/HEAD".to_string()];
    patterns.extend(
        REMOTE_BASE_CANDIDATES
            .iter()
            .map(|candidate| format!("refs/remotes/{candidate}")),
    );
    patterns.extend(
        LOCAL_BASE_CANDIDATES
            .iter()
            .map(|candidate| format!("refs/heads/{candidate}")),
    );

    let mut args = vec![
        "for-each-ref".to_string(),
        "--format=%(refname)\t%(objectname)\t%(symref)".to_string(),
    ];
    args.extend(patterns);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    let Some(listing) = git_stdout(repository, &args) else {
        return BaseBranches::default();
    };

    // name -> tip, plus whatever `origin/HEAD` turned out to point at.
    let mut tips: HashMap<String, String> = HashMap::new();
    let mut origin_head: Option<(String, String)> = None;

    for line in listing.lines() {
        let mut fields = line.split('\t');
        let (Some(name), Some(object)) = (fields.next(), fields.next()) else {
            continue;
        };
        let symref = fields.next().unwrap_or("").trim();
        let (name, object) = (name.trim(), object.trim());
        if object.is_empty() {
            // A symbolic ref whose target does not exist. Git lists the pointer but has no
            // commit to report, and a base nothing resolves to is no base at all.
            continue;
        }

        if name == "refs/remotes/origin/HEAD" {
            if let Some(target) = shorten_ref(symref) {
                origin_head = Some((target, object.to_string()));
            }
            continue;
        }
        if let Some(name) = shorten_ref(name) {
            tips.insert(name, object.to_string());
        }
    }

    let mut branches: Vec<String> = Vec::new();
    let mut resolved: Vec<String> = Vec::new();
    let push = |name: String, tip: String, branches: &mut Vec<String>, resolved: &mut Vec<String>| {
        if !branches.contains(&name) {
            branches.push(name);
            resolved.push(tip);
        }
    };

    if let Some((name, tip)) = origin_head {
        push(name, tip, &mut branches, &mut resolved);
    }
    for candidate in REMOTE_BASE_CANDIDATES {
        if let Some(tip) = tips.get(candidate) {
            push(candidate.to_string(), tip.clone(), &mut branches, &mut resolved);
        }
    }

    if !branches.is_empty() {
        return BaseBranches {
            branches,
            tips: resolved,
            used_local_fallback: false,
        };
    }

    for candidate in LOCAL_BASE_CANDIDATES {
        if let Some(tip) = tips.get(candidate) {
            push(candidate.to_string(), tip.clone(), &mut branches, &mut resolved);
        }
    }

    let used_local_fallback = !branches.is_empty();
    BaseBranches {
        branches,
        tips: resolved,
        used_local_fallback,
    }
}

fn is_ancestor(repository: &Path, ancestor: &str, descendant: &str) -> Result<bool, String> {
    let output = git_command(repository)
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
fn is_squash_merged(
    repository: &Path,
    head: &str,
    base: &str,
    scratch: &ScratchObjectDir,
) -> bool {
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
    let mut probe = git_command(repository);
    probe
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

    let cherry = git_command(repository)
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
    let output = git_command(worktree)
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

/// The shared `.git` directory behind `directory`, without spawning Git for the common case.
///
/// This runs once per *discovered* directory, and a tree of twenty worktrees discovers twenty
/// of them — so on a large folder it was the single largest source of process spawns, all of
/// them asking Git something the filesystem already says out loud:
///
/// * a main worktree has `.git` as a **directory**, and that directory *is* the common dir;
/// * a linked worktree has `.git` as a **file** holding `gitdir: <path>`, and that git dir
///   holds a `commondir` file pointing back at the shared one (normally `../..`).
///
/// Anything that does not match — a submodule, a `.git` file without `commondir`, an unreadable
/// path — falls through to asking Git, so unusual layouts behave exactly as they did before.
///
/// The result is canonicalised because its only job is to be a dedup key: a linked worktree
/// records the physical path Git stored at creation, while the main worktree's is derived from
/// the walk, and on macOS those differ by the `/var` → `/private/var` symlink alone. Two
/// spellings of one repository would make it scan twice.
fn git_common_dir(directory: &Path) -> Option<PathBuf> {
    common_dir_from_git_entry(directory)
        .and_then(|path| fs::canonicalize(path).ok())
        .or_else(|| {
            git_stdout(
                directory,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            )
            .map(PathBuf::from)
            .and_then(|path| fs::canonicalize(&path).ok().or(Some(path)))
        })
}

fn common_dir_from_git_entry(directory: &Path) -> Option<PathBuf> {
    let entry = directory.join(".git");
    let metadata = fs::symlink_metadata(&entry).ok()?;

    if metadata.is_dir() {
        return looks_like_git_dir(entry);
    }
    if !metadata.is_file() {
        return None;
    }

    let contents = fs::read_to_string(&entry).ok()?;
    let recorded = contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))?
        .trim();
    if recorded.is_empty() {
        return None;
    }

    // A `gitdir:` path is usually absolute, but Git accepts a relative one, resolved against
    // the worktree that holds the file.
    let git_dir = {
        let recorded = Path::new(recorded);
        if recorded.is_absolute() {
            recorded.to_path_buf()
        } else {
            directory.join(recorded)
        }
    };

    // `commondir` is what makes this reliable rather than a guess about directory depth: it is
    // written by Git itself and stays correct for layouts that do not look like
    // `.git/worktrees/<name>`.
    let common = fs::read_to_string(git_dir.join("commondir")).ok()?;
    let common = common.trim();
    if common.is_empty() {
        return None;
    }

    let common = Path::new(common);
    looks_like_git_dir(if common.is_absolute() {
        common.to_path_buf()
    } else {
        git_dir.join(common)
    })
}

/// A `.git` directory is only evidence of a repository if Git would also accept it as one.
///
/// The shortcut above reads layout, not validity, so on its own it would promote a bare empty
/// `.git` folder to a real repository — and the scan would then try to fetch it and report the
/// failure twice. `HEAD` is the file Git itself looks for, and its absence sends the caller
/// back to asking Git, which answers the same "no" it always did.
fn looks_like_git_dir(candidate: PathBuf) -> Option<PathBuf> {
    candidate.join("HEAD").exists().then_some(candidate)
}

/// `git fetch`, but it can never sit forever on a credential prompt and can never outlive the
/// timeout. Both matter because the scan fetches every discovered repository.
fn fetch_repository(repository: &Path) -> Result<(), String> {
    let mut child = git_command(repository)
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

/// What a tick concluded about one worktree, and the refs that conclusion rested on.
///
/// The expensive half of a scan is deciding that a worktree is *not* merged: a match stops the
/// search at the first base that fits, while a miss pays an ancestor check and a full squash
/// probe against every base — around thirty `git` processes. Those are exactly the worktrees a
/// developer keeps around, so without a memory the background tick re-derives the same "no"
/// for the same unchanged branches every fifteen minutes, forever.
///
/// A verdict only depends on two things: the commit the worktree is on, and the commits the
/// bases are on. While all of those are unchanged the previous answer is still the right one,
/// so the negative results are cached as eagerly as the positive ones.
///
/// What is deliberately *not* cached is whether the worktree is dirty. Files change without any
/// ref moving, so `is_dirty` has to be re-read every tick — but only for worktrees that matched,
/// which is a small set and precisely the ones about to be offered for deletion.
#[derive(Debug, Default)]
pub(crate) struct VerdictCache {
    entries: HashMap<VerdictKey, CachedVerdict>,
    seen: HashSet<VerdictKey>,
    hits: u64,
    misses: u64,
}

type VerdictKey = (PathBuf, PathBuf);

#[derive(Debug, Clone, PartialEq, Eq)]
struct CachedVerdict {
    head: String,
    base_tips: Vec<String>,
    verdict: Verdict,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Verdict {
    Matched { base: String, state: &'static str },
    Unmerged,
}

impl VerdictCache {
    /// Start a pass. Anything not looked up before [`VerdictCache::end_pass`] is dropped, so a
    /// deleted worktree does not sit in the map for the life of the process.
    pub(crate) fn begin_pass(&mut self) {
        self.seen.clear();
        self.hits = 0;
        self.misses = 0;
    }

    pub(crate) fn end_pass(&mut self) {
        let seen = std::mem::take(&mut self.seen);
        self.entries.retain(|key, _| seen.contains(key));
    }

    fn lookup(
        &mut self,
        repository: &Path,
        worktree: &Path,
        head: &str,
        bases: &BaseBranches,
    ) -> Option<Verdict> {
        let key = (repository.to_path_buf(), worktree.to_path_buf());
        self.seen.insert(key.clone());

        let entry = self.entries.get(&key)?;
        if entry.head == head && entry.base_tips == bases.tips {
            self.hits += 1;
            return Some(entry.verdict.clone());
        }
        None
    }

    fn store(
        &mut self,
        repository: &Path,
        worktree: &Path,
        head: &str,
        bases: &BaseBranches,
        verdict: Verdict,
    ) {
        self.misses += 1;
        self.entries.insert(
            (repository.to_path_buf(), worktree.to_path_buf()),
            CachedVerdict {
                head: head.to_string(),
                base_tips: bases.tips.clone(),
                verdict,
            },
        );
    }

    pub(crate) fn hits(&self) -> u64 {
        self.hits
    }

    pub(crate) fn considered(&self) -> u64 {
        self.hits + self.misses
    }
}

/// Candidates only, for the callers that do not report which bases were used.
fn collect_merged_worktrees(
    repository: &Path,
    cache: Option<&mut VerdictCache>,
) -> Result<Vec<MergedWorktree>, String> {
    collect_with_bases(repository, cache).map(|(candidates, _)| candidates)
}

/// Candidates plus the bases they were compared against.
///
/// The full scan reports those bases to the user, and used to resolve them a second time to do
/// it — once here and once in the caller, for every repository including the ones that turn out
/// to have no linked worktrees at all. Returning them means the question is asked once, and only
/// where the answer is actually needed.
fn collect_with_bases(
    repository: &Path,
    mut cache: Option<&mut VerdictCache>,
) -> Result<(Vec<MergedWorktree>, Option<BaseBranches>), String> {
    let output = git_command(repository)
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
        return Ok((Vec::new(), None));
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
    let mut scratch: Option<ScratchObjectDir> = None;

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
        //
        // "Gone" has to mean gone. `Path::exists` and Git's own prunable check both read an
        // unreadable directory as missing, and treating that as stale would prune the
        // registration of a worktree whose files are all still there. An unreadable worktree
        // falls through instead, where its status read fails closed and removal reports why.
        let is_stale = match record.path.try_exists() {
            Ok(false) => true,
            Ok(true) => record.prunable_reason.is_some(),
            Err(_) => false,
        };
        if is_stale {
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

        let verdict = match cache
            .as_deref_mut()
            .and_then(|cache| cache.lookup(repository, &record.path, &record.head, &bases))
        {
            Some(verdict) => verdict,
            None => {
                let mut matched = Verdict::Unmerged;
                for base in &bases.branches {
                    // An error here is a broken repository rather than an answer, so it
                    // propagates instead of being remembered as "not merged".
                    if is_ancestor(repository, &record.head, base)? {
                        matched = Verdict::Matched {
                            base: base.clone(),
                            state: STATE_MERGED,
                        };
                        break;
                    }
                    // Created on the first probe this repository needs and reused for the
                    // rest: it used to be made and torn down around every single probe, which
                    // on a full scan is a few hundred create/write/delete cycles in the temp
                    // directory for work that fits in one.
                    if scratch.is_none() {
                        scratch = ScratchObjectDir::new();
                    }
                    let squashed = scratch.as_ref().is_some_and(|scratch| {
                        is_squash_merged(repository, &record.head, base, scratch)
                    });
                    if squashed {
                        matched = Verdict::Matched {
                            base: base.clone(),
                            state: STATE_SQUASHED,
                        };
                        break;
                    }
                }
                if let Some(cache) = cache.as_deref_mut() {
                    cache.store(repository, &record.path, &record.head, &bases, matched.clone());
                }
                matched
            }
        };

        let Verdict::Matched { base: base_branch, state } = verdict else {
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

    Ok((candidates, Some(bases)))
}

fn registered_worktree_paths(repository: &Path) -> Vec<PathBuf> {
    let Ok(output) = git_command(repository)
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
        already_gone: false,
    }
}

fn removed(path: String) -> WorktreeDeleteResult {
    WorktreeDeleteResult {
        success: true,
        path,
        error: None,
        already_gone: false,
    }
}

/// Exactly one `-f`, never two. A second `-f` is what lets git remove a *locked* worktree, so
/// keeping it at one means the junk retry below can never bypass a lock — including one taken
/// between the lock check and this call.
fn run_worktree_remove(repository: &Path, worktree: &Path, force: bool) -> Result<(), String> {
    let mut command = git_command(repository);
    command.args(["worktree", "remove"]);
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
///
/// `force` is the user's consent to lose uncommitted changes. It is deliberately narrow: it
/// never outweighs a newer commit, a lock, or a worktree that is no longer merged, because the
/// warning the user agreed to described none of those.
fn remove_prepared_worktree(
    repository: &Path,
    worktree: &Path,
    expected_head: Option<&str>,
    force: bool,
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
        if is_registered(repository, worktree) {
            return failed_removal(path, "No longer merged into the repository's base branches");
        }
        // Neither registered nor on disk: whoever removed it did exactly what was asked.
        // A directory that is still there but unregistered is not a worktree any more, and
        // whatever it holds now is not ours to delete.
        if !worktree.exists() {
            return WorktreeDeleteResult {
                already_gone: true,
                ..removed(path)
            };
        }
        return failed_removal(path, "No longer registered");
    };

    if candidate.state == STATE_STALE {
        return prune_stale_worktree(repository, worktree);
    }

    let requested_path = match worktree.canonicalize() {
        Ok(requested_path) => requested_path,
        // Deleted since the repository was re-read a moment ago. Only a missing path means
        // "gone"; anything else — a permission error, say — leaves the worktree in place and has
        // to be reported, not claimed as a removal.
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            if is_registered(repository, worktree) {
                // The directory went but the registration stayed: that is a stale entry now.
                return prune_stale_worktree(repository, worktree);
            }
            return WorktreeDeleteResult {
                already_gone: true,
                ..removed(path)
            };
        }
        Err(error) => return failed_removal(path, format!("Cannot read worktree: {error}")),
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
    if status.is_dirty && !force {
        return failed_removal(path, "Worktree has uncommitted changes");
    }

    match run_worktree_remove(repository, &requested_path, false) {
        Ok(()) => removed(path),
        Err(error) => {
            // Untracked junk makes git refuse, and a watcher cookie can appear between the
            // status read above and the removal. Re-read rather than trusting either snapshot:
            // without consent, force only when nothing but ignored content and junk is present.
            if !force && worktree_status(worktree).is_dirty {
                return failed_removal(path, error);
            }
            match run_worktree_remove(repository, &requested_path, true) {
                Ok(()) => removed(path),
                Err(force_error) => failed_removal(path, force_error),
            }
        }
    }
}

/// Clear a registration whose directory is gone. `git worktree prune` only touches metadata in
/// `.git/worktrees`, so it cannot destroy anything on disk.
fn prune_stale_worktree(repository: &Path, worktree: &Path) -> WorktreeDeleteResult {
    let path = worktree.to_string_lossy().to_string();
    let _ = git_command(repository)
        .args(["worktree", "prune"])
        .output();

    if is_registered(repository, worktree) {
        failed_removal(path, "Git did not prune this stale worktree entry")
    } else {
        removed(path)
    }
}

/// Single-worktree removal that collects the repository view itself. The batch command shares
/// one collection across a repository instead, so this exists only for tests that exercise one
/// removal in isolation.
#[cfg(test)]
fn remove_merged_worktree(repository: &Path, worktree: &Path) -> WorktreeDeleteResult {
    match collect_merged_worktrees(repository, None) {
        Ok(candidates) => remove_prepared_worktree(repository, worktree, None, false, &candidates),
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
    //
    // Fetching is network-bound, so it is capped rather than spread across the whole rayon pool:
    // a folder with a hundred repositories would otherwise open a hundred connections at once,
    // each holding a twenty-second timeout, and saturate the link competing with itself.
    let fetch_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(FETCH_CONCURRENCY)
        .build();
    let fetch = || {
        repositories
            .par_iter()
            .filter(|(_, resolvable)| *resolvable)
            .filter_map(|(repository, _)| {
                fetch_repository(repository)
                    .err()
                    .map(|error| format!("{}: could not fetch, results may be stale ({error})", repository.display()))
            })
            .collect::<Vec<String>>()
    };
    let mut warnings: Vec<String> = match &fetch_pool {
        Ok(pool) => pool.install(fetch),
        // A pool that cannot be built is no reason to skip the fetch; the default pool is
        // simply wider than intended.
        Err(_) => fetch(),
    };

    let mut diagnostics = Vec::new();
    let mut evaluated_repositories = 0;
    let mut repository_errors = Vec::new();
    let mut seen = HashSet::new();
    let mut worktrees = Vec::new();

    for (repository, _resolvable) in repositories {
        let candidates = match collect_with_bases(&repository, None) {
            Ok((candidates, bases)) => {
                evaluated_repositories += 1;
                // Reported for the repositories that were actually compared. One with no linked
                // worktrees never reaches a comparison, and claiming a base for it would be
                // describing work that did not happen.
                if let Some(bases) = bases.filter(|bases| !bases.branches.is_empty()) {
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
        .filter(|worktree| is_selectable(worktree))
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

/// Whether the window lets someone tick this row. A lock is the one thing that rules it out:
/// it is an explicit "hands off" left by a person or tool, whereas uncommitted changes are
/// offered behind a warning the user has to accept.
fn is_selectable(worktree: &MergedWorktree) -> bool {
    !worktree.is_locked
}

/// What the menu bar counts: every row the window would let you select, minus stale entries,
/// which free nothing and are only leftover Git bookkeeping. `useCleanup.ts` mirrors this rule
/// to notice when its list and the menu bar disagree, so the two must change together.
fn counts_as_removable(worktree: &MergedWorktree) -> bool {
    is_selectable(worktree) && worktree.state != STATE_STALE
}

/// How many worktrees under `root` can be removed right now — some of them only after the user
/// accepts losing their uncommitted changes.
///
/// This is the number the menu bar shows, and it deliberately goes through the same
/// `collect_merged_worktrees` and the same [`counts_as_removable`] rule the window uses. It skips the two
/// expensive steps of a full scan: no `fetch_repository` (a background tick must not touch the
/// network) and no `calculate_dir_size` (walking `node_modules` is by far the slowest part and a
/// count does not need bytes).
pub(crate) fn count_removable_worktrees(root: &Path, cache: &mut VerdictCache) -> usize {
    if !root.is_dir() {
        return 0;
    }

    let mut seen = HashSet::new();
    let mut removable = 0;

    for (repository, resolvable) in unique_repositories(discover_git_repositories(root)) {
        if !resolvable {
            continue;
        }
        let Ok(candidates) = collect_merged_worktrees(&repository, Some(cache)) else {
            continue;
        };
        for candidate in candidates {
            if !seen.insert((candidate.repository_path.clone(), candidate.path.clone())) {
                continue;
            }
            if counts_as_removable(&candidate) {
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

        let candidates = match collect_merged_worktrees(&repository, None) {
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
                removal.force,
                &candidates,
            );
            removed_any |= result.success;
            results[index] = Some(result);
        }

        if removed_any || !stale_indices.is_empty() {
            let _ = git_command(&repository)
                .args(["worktree", "prune"])
                .output();
        }

        for index in stale_indices {
            let worktree = Path::new(&removals[index].worktree_path);
            let pruned = !is_registered(&repository, worktree);
            results[index] = Some(if pruned {
                removed(removals[index].worktree_path.clone())
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
        BaseBranches, Verdict, VerdictCache,
        is_junk_entry,
        is_squash_merged, parse_worktree_porcelain, remove_merged_worktree, ScratchObjectDir,
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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

        assert!(candidates.is_empty());
    }

    #[test]
    fn returns_merged_worktree_and_marks_uncommitted_changes_as_dirty() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/done");
        repo.commit_file(&worktree, "done.txt");
        repo.git(&["merge", "--ff-only", "feature/done"]);

        let clean_candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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
        let dirty_candidates = collect_merged_worktrees(repo.path(), None).expect("rescan worktrees");

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
        // Dirty worktrees can be removed with consent, so they count towards what is reclaimable.
        assert_eq!(result.total_size, clean.size + dirty.size);
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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let scratch = ScratchObjectDir::new().expect("scratch object directory");
        assert!(is_squash_merged(
            repo.path(),
            &repo.git(&["rev-parse", "feature/probe"]),
            "main",
            &scratch
        ));

        assert_eq!(repo.git(&["count-objects", "-v"]), loose_before);
    }

    #[test]
    fn does_not_treat_unrelated_branch_content_as_squash_merged() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/never-merged");
        repo.commit_file(&worktree, "open.txt");

        let scratch = ScratchObjectDir::new().expect("scratch object directory");
        assert!(!is_squash_merged(
            repo.path(),
            &repo.git(&["rev-parse", "feature/never-merged"]),
            "main",
            &scratch
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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

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

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

        assert!(candidates.is_empty());
    }

    #[test]
    fn reports_a_missing_worktree_directory_as_stale_and_prunes_it() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/stale");
        repo.git(&["merge", "--ff-only", "feature/stale"]);
        fs::remove_dir_all(&worktree).expect("simulate a hand-deleted worktree directory");

        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].state, "stale");
        assert!(!candidates[0].is_dirty, "a missing directory is not dirty work");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: candidates[0].repository_path.clone(),
            worktree_path: candidates[0].path.clone(),
            head: candidates[0].head.clone(),
            force: false,
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
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert_eq!(candidates.len(), 1);

        // Another session commits into the worktree between the scan and the removal.
        repo.commit_file(&worktree, "new-work.txt");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: candidates[0].repository_path.clone(),
            worktree_path: candidates[0].path.clone(),
            head: candidates[0].head.clone(),
            force: false,
        }]);

        assert!(!results[0].success);
        assert!(results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Changed since scan")));
        assert!(worktree.exists());
    }

    #[test]
    fn treats_a_worktree_removed_elsewhere_as_already_gone() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/removed-elsewhere");
        repo.git(&["merge", "--ff-only", "feature/removed-elsewhere"]);
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert_eq!(candidates.len(), 1);

        // Another session removes it between the scan and the click — on a machine where agents
        // clean up after every merge this is the normal case, not an edge case.
        repo.git(&[
            "worktree",
            "remove",
            worktree.to_str().expect("UTF-8 fixture path"),
        ]);

        let results = delete_merged_worktrees_in(vec![removal(&candidates[0], false)]);

        assert!(results[0].success, "{}", results[0].error.clone().unwrap_or_default());
        assert!(results[0].already_gone);
        assert_eq!(results[0].error, None);
    }

    #[test]
    fn an_unreadable_worktree_is_a_failure_not_already_gone() {
        use std::os::unix::fs::PermissionsExt;

        let repo = TestRepo::new();
        let parent = repo.path().join("sealed");
        fs::create_dir_all(&parent).expect("create parent directory");
        repo.git(&["branch", "feature/sealed"]);
        let worktree = parent.join("worktree");
        repo.git(&[
            "worktree",
            "add",
            worktree.to_str().expect("UTF-8 fixture path"),
            "feature/sealed",
        ]);
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        let candidate = candidates
            .iter()
            .find(|candidate| candidate.branch == "feature/sealed")
            .expect("sealed worktree candidate")
            .clone();

        // Still there and still registered — only unreachable. That is an error to report,
        // never a removal to claim.
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o000)).expect("seal parent");
        let results = delete_merged_worktrees_in(vec![removal(&candidate, true)]);
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o755)).expect("unseal parent");

        assert!(!results[0].success);
        assert!(!results[0].already_gone);
        assert!(worktree.exists());
    }

    #[test]
    fn still_refuses_an_unregistered_directory_that_exists() {
        let repo = TestRepo::new();
        let bystander = repo.path().join("not-a-worktree");
        fs::create_dir_all(&bystander).expect("create bystander directory");
        fs::write(bystander.join("keep.txt"), "keep me\n").expect("write bystander file");

        let results = delete_merged_worktrees_in(vec![WorktreeRemoval {
            repository_path: repo.path().to_string_lossy().to_string(),
            worktree_path: bystander.to_string_lossy().to_string(),
            head: String::new(),
            force: true,
        }]);

        assert!(!results[0].success);
        assert!(!results[0].already_gone);
        assert_eq!(results[0].error.as_deref(), Some("No longer registered"));
        assert!(bystander.join("keep.txt").exists());
    }

    #[test]
    fn removes_a_dirty_worktree_the_user_agreed_to_lose() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/scratch-left-behind");
        repo.commit_file(&worktree, "done.txt");
        repo.git(&["merge", "--ff-only", "feature/scratch-left-behind"]);
        fs::create_dir_all(worktree.join(".pi")).expect("create agent scratch dir");
        fs::write(worktree.join(".pi").join("session.json"), "{}\n").expect("write scratch");
        fs::write(worktree.join("done.txt"), "edited\n").expect("modify tracked file");
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert!(candidates[0].is_dirty);

        let results = delete_merged_worktrees_in(vec![removal(&candidates[0], true)]);

        assert!(results[0].success, "{}", results[0].error.clone().unwrap_or_default());
        assert!(!results[0].already_gone);
        assert!(!worktree.exists());
        repo.git(&[
            "show-ref",
            "--verify",
            "--quiet",
            "refs/heads/feature/scratch-left-behind",
        ]);
    }

    #[test]
    fn refuses_a_dirty_worktree_nobody_agreed_to_lose() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/went-dirty");
        repo.git(&["merge", "--ff-only", "feature/went-dirty"]);
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert!(!candidates[0].is_dirty, "clean at scan time, so no consent was asked");

        fs::write(worktree.join("new-work.txt"), "keep me\n").expect("write new work");
        let results = delete_merged_worktrees_in(vec![removal(&candidates[0], false)]);

        assert!(!results[0].success);
        assert!(results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("uncommitted changes")));
        assert!(worktree.join("new-work.txt").exists());
    }

    #[test]
    fn consent_to_lose_changes_never_covers_a_newer_commit() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/committed-after");
        repo.git(&["merge", "--ff-only", "feature/committed-after"]);
        fs::write(worktree.join("scratch.txt"), "scratch\n").expect("write scratch");
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");
        assert!(candidates[0].is_dirty);

        repo.commit_file(&worktree, "new-work.txt");
        let results = delete_merged_worktrees_in(vec![removal(&candidates[0], true)]);

        assert!(!results[0].success);
        assert!(results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("Changed since scan")));
        assert!(worktree.exists());
    }

    #[test]
    fn consent_to_lose_changes_never_overrides_a_lock() {
        let repo = TestRepo::new();
        let worktree = repo.add_feature_worktree("feature/locked-dirty");
        repo.git(&["merge", "--ff-only", "feature/locked-dirty"]);
        fs::write(worktree.join("scratch.txt"), "scratch\n").expect("write scratch");
        repo.git(&[
            "worktree",
            "lock",
            "--reason",
            "held by another tool",
            worktree.to_str().expect("UTF-8 fixture path"),
        ]);
        let candidates = collect_merged_worktrees(repo.path(), None).expect("scan worktrees");

        let results = delete_merged_worktrees_in(vec![removal(&candidates[0], true)]);

        assert!(!results[0].success);
        assert!(results[0]
            .error
            .as_deref()
            .is_some_and(|error| error.contains("locked")));
        assert!(worktree.join("scratch.txt").exists());
    }

    fn removal(candidate: &super::MergedWorktree, force: bool) -> WorktreeRemoval {
        WorktreeRemoval {
            repository_path: candidate.repository_path.clone(),
            worktree_path: candidate.path.clone(),
            head: candidate.head.clone(),
            force,
        }
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
        let candidates = collect_merged_worktrees(&root, None).expect("repository without worktrees");

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

        // Five merged-or-not worktrees. The clean one and the dirty one can both be selected in
        // the window; the locked, stale and unmerged ones cannot, so the menu bar skips them too.
        assert_eq!(
            count_removable_worktrees(&scan_root, &mut VerdictCache::default()),
            2
        );
        assert!(removable.exists());

        drop(repo);
        let _ = fs::remove_dir_all(scan_root);
    }

    #[test]
    fn the_menu_bar_count_is_zero_for_a_folder_that_is_not_there() {
        let missing = std::env::temp_dir().join("node-modules-cleaner-definitely-missing");

        assert_eq!(
            count_removable_worktrees(&missing, &mut VerdictCache::default()),
            0
        );
    }

    /// Print what the scan finds on the machine it runs on, against a real directory tree.
    ///
    /// `WORKTREE_SCAN_PATH=~/Projects cargo test report_real_worktrees -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn report_tick_cost() {
        // What the menu bar refresh actually costs against a real projects folder, cold and
        // then warm. Run with:
        //   cargo test --release -- --ignored report_tick_cost --nocapture
        use super::{git_spawn_count, with_background_git};
        use std::time::Instant;

        let scan_path = std::env::var("WORKTREE_SCAN_PATH").unwrap_or_else(|_| {
            format!("{}/Projects", std::env::var("HOME").unwrap_or_default())
        });
        let root = PathBuf::from(&scan_path);

        // Both policies, because they are not the same workload: the tray runs every Git child
        // at background priority, which throttles I/O as well as CPU.
        for (label, background) in [("foreground", false), ("background", true)] {
            let mut cache = VerdictCache::default();
            for pass in 1..=3 {
                let before = git_spawn_count();
                let started = Instant::now();
                let mut run = || {
                    cache.begin_pass();
                    let removable = count_removable_worktrees(&root, &mut cache);
                    cache.end_pass();
                    removable
                };
                let removable = if background {
                    with_background_git(run)
                } else {
                    run()
                };
                eprintln!(
                    "{label} pass {pass}: {:.2}s, {} git spawns, cache {}/{} hit, {removable} removable",
                    started.elapsed().as_secs_f64(),
                    git_spawn_count() - before,
                    cache.hits(),
                    cache.considered(),
                );
            }
        }
    }

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

    fn bases(tips: &[&str]) -> BaseBranches {
        BaseBranches {
            branches: vec!["origin/main".to_string()],
            tips: tips.iter().map(|tip| tip.to_string()).collect(),
            used_local_fallback: false,
        }
    }

    #[test]
    fn an_unchanged_worktree_reuses_the_previous_verdict() {
        // The whole point of the cache: deciding a branch is *not* merged costs about thirty
        // git processes, and nothing about that answer changes while the commits do not.
        let mut cache = VerdictCache::default();
        let repository = Path::new("/repo");
        let worktree = Path::new("/repo/feature");

        cache.begin_pass();
        assert_eq!(
            cache.lookup(repository, worktree, "head1", &bases(&["base1"])),
            None
        );
        cache.store(
            repository,
            worktree,
            "head1",
            &bases(&["base1"]),
            Verdict::Unmerged,
        );
        cache.end_pass();

        cache.begin_pass();
        assert_eq!(
            cache.lookup(repository, worktree, "head1", &bases(&["base1"])),
            Some(Verdict::Unmerged)
        );
        assert_eq!(cache.hits(), 1);
    }

    #[test]
    fn a_moved_head_invalidates_the_verdict() {
        let mut cache = VerdictCache::default();
        let repository = Path::new("/repo");
        let worktree = Path::new("/repo/feature");

        cache.begin_pass();
        cache.store(
            repository,
            worktree,
            "head1",
            &bases(&["base1"]),
            Verdict::Unmerged,
        );
        cache.end_pass();

        cache.begin_pass();
        assert_eq!(
            cache.lookup(repository, worktree, "head2", &bases(&["base1"])),
            None
        );
    }

    #[test]
    fn a_moved_base_invalidates_the_verdict() {
        // The case that makes head-only keying wrong: the branch stands still, the base moves
        // past it, and a worktree that was open a minute ago is now merged.
        let mut cache = VerdictCache::default();
        let repository = Path::new("/repo");
        let worktree = Path::new("/repo/feature");

        cache.begin_pass();
        cache.store(
            repository,
            worktree,
            "head1",
            &bases(&["base1"]),
            Verdict::Unmerged,
        );
        cache.end_pass();

        cache.begin_pass();
        assert_eq!(
            cache.lookup(repository, worktree, "head1", &bases(&["base2"])),
            None
        );
    }

    #[test]
    fn a_worktree_that_disappears_is_forgotten() {
        let mut cache = VerdictCache::default();
        let repository = Path::new("/repo");
        let worktree = Path::new("/repo/feature");

        cache.begin_pass();
        cache.store(
            repository,
            worktree,
            "head1",
            &bases(&["base1"]),
            Verdict::Unmerged,
        );
        cache.end_pass();

        // A pass that never asks about it: the worktree is gone, and so is its entry.
        cache.begin_pass();
        cache.end_pass();

        cache.begin_pass();
        assert_eq!(
            cache.lookup(repository, worktree, "head1", &bases(&["base1"])),
            None
        );
    }

    #[test]
    fn a_cached_pass_reports_the_same_worktrees_as_an_uncached_one() {
        // The cache is allowed to remember verdicts, never to change them.
        let repo = TestRepo::new();
        repo.add_feature_worktree("merged-branch");
        repo.add_feature_worktree("open-branch");
        let worktree = repo.path().join("open-branch");
        repo.commit_file(&worktree, "work.txt");

        let uncached = collect_merged_worktrees(repo.path(), None).expect("uncached scan");

        let mut cache = VerdictCache::default();
        cache.begin_pass();
        let cold = collect_merged_worktrees(repo.path(), Some(&mut cache)).expect("cold scan");
        cache.end_pass();

        cache.begin_pass();
        let warm = collect_merged_worktrees(repo.path(), Some(&mut cache)).expect("warm scan");
        cache.end_pass();

        assert_eq!(uncached, cold);
        assert_eq!(cold, warm);
        // The second pass answered entirely from memory.
        assert_eq!(cache.hits(), cache.considered());
        assert!(cache.considered() > 0);
    }
}
