use crate::commands::calculate_dir_size;
use serde::{Deserialize, Serialize};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
struct WorktreeRecord {
    path: PathBuf,
    head: String,
    branch: Option<String>,
    is_detached: bool,
    is_bare: bool,
    lock_reason: Option<String>,
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
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeScanResult {
    pub worktrees: Vec<MergedWorktree>,
    pub total_size: u64,
    pub scan_path: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WorktreeRemoval {
    pub repository_path: String,
    pub worktree_path: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorktreeDeleteResult {
    pub success: bool,
    pub path: String,
    pub error: Option<String>,
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
            }
        }
    }

    if let Some(record) = current {
        records.push(record);
    }

    records
}

fn find_default_branch(repository: &Path) -> Option<String> {
    let origin_head = Command::new("git")
        .arg("-C")
        .arg(repository)
        .args([
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|branch| branch.trim().to_string())
        .filter(|branch| !branch.is_empty());

    if let Some(branch) = origin_head {
        let resolves_to_commit = Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{branch}^{{commit}}"))
            .output()
            .is_ok_and(|output| output.status.success());
        if resolves_to_commit {
            return Some(branch);
        }
    }

    ["main", "master"].into_iter().find_map(|branch| {
        Command::new("git")
            .arg("-C")
            .arg(repository)
            .args(["show-ref", "--verify", "--quiet"])
            .arg(format!("refs/heads/{branch}"))
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|_| branch.to_string())
    })
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

fn collect_merged_worktrees(repository: &Path) -> Result<Vec<MergedWorktree>, String> {
    let base_branch = find_default_branch(repository)
        .ok_or_else(|| "Could not determine the repository's default branch".to_string())?;
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
    let repository_name = repository_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Repository")
        .to_string();
    let mut candidates = Vec::new();

    for record in records.into_iter().skip(1) {
        let Some(branch) = record.branch else {
            continue;
        };
        if record.is_bare || record.is_detached || record.head.is_empty() {
            continue;
        }

        if !is_ancestor(repository, &record.head, &base_branch)? {
            continue;
        }

        let status = Command::new("git")
            .arg("-C")
            .arg(&record.path)
            .args([
                "status",
                "--porcelain",
                "--ignored=matching",
                "--untracked-files=normal",
            ])
            .output();
        let (is_dirty, has_ignored_files) = match status {
            Ok(output) if output.status.success() => {
                let lines: Vec<&[u8]> = output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .collect();
                (
                    lines.iter().any(|line| !line.starts_with(b"!! ")),
                    lines.iter().any(|line| line.starts_with(b"!! ")),
                )
            }
            _ => (true, false),
        };

        candidates.push(MergedWorktree {
            path: record.path.to_string_lossy().to_string(),
            branch,
            repository_path: repository_path.to_string_lossy().to_string(),
            repository_name: repository_name.clone(),
            base_branch: base_branch.clone(),
            size: 0,
            is_dirty,
            has_ignored_files,
            is_locked: record.lock_reason.is_some(),
            lock_reason: record.lock_reason.filter(|reason| !reason.is_empty()),
        });
    }

    Ok(candidates)
}

fn remove_merged_worktree(repository: &Path, worktree: &Path) -> WorktreeDeleteResult {
    let path = worktree.to_string_lossy().to_string();
    let requested_path = match worktree.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            return WorktreeDeleteResult {
                success: false,
                path,
                error: Some(format!("Could not resolve worktree path: {error}")),
            };
        }
    };

    let candidates = match collect_merged_worktrees(repository) {
        Ok(candidates) => candidates,
        Err(error) => {
            return WorktreeDeleteResult {
                success: false,
                path,
                error: Some(error),
            };
        }
    };
    let candidate = candidates.into_iter().find(|candidate| {
        Path::new(&candidate.path)
            .canonicalize()
            .is_ok_and(|candidate_path| candidate_path == requested_path)
    });

    let Some(candidate) = candidate else {
        return WorktreeDeleteResult {
            success: false,
            path,
            error: Some("Worktree is no longer registered or merged".to_string()),
        };
    };
    if candidate.is_dirty {
        return WorktreeDeleteResult {
            success: false,
            path,
            error: Some("Worktree has uncommitted changes".to_string()),
        };
    }
    if candidate.is_locked {
        let detail = candidate
            .lock_reason
            .map(|reason| format!(": {reason}"))
            .unwrap_or_default();
        return WorktreeDeleteResult {
            success: false,
            path,
            error: Some(format!("Worktree is locked{detail}")),
        };
    }

    match Command::new("git")
        .arg("-C")
        .arg(repository)
        .args(["worktree", "remove", "--"])
        .arg(&requested_path)
        .output()
    {
        Ok(output) if output.status.success() => WorktreeDeleteResult {
            success: true,
            path,
            error: None,
        },
        Ok(output) => WorktreeDeleteResult {
            success: false,
            path,
            error: Some(String::from_utf8_lossy(&output.stderr).trim().to_string()),
        },
        Err(error) => WorktreeDeleteResult {
            success: false,
            path,
            error: Some(format!("Failed to run git: {error}")),
        },
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

fn scan_for_merged_worktrees_in(scan_path: &Path) -> Result<WorktreeScanResult, String> {
    if !scan_path.exists() {
        return Err("Path does not exist".to_string());
    }
    if !scan_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    let repositories = discover_git_repositories(scan_path);
    let repository_count = repositories.len();
    let mut evaluated_repositories = 0;
    let mut repository_errors = Vec::new();
    let mut seen = HashSet::new();
    let mut worktrees = Vec::new();
    for repository in repositories {
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

    Ok(WorktreeScanResult {
        worktrees,
        total_size,
        scan_path: scan_path.to_string_lossy().to_string(),
        warnings: repository_errors,
    })
}

#[tauri::command]
pub async fn scan_for_merged_worktrees(path: String) -> Result<WorktreeScanResult, String> {
    scan_for_merged_worktrees_in(Path::new(&path))
}

#[tauri::command]
pub async fn delete_merged_worktrees(removals: Vec<WorktreeRemoval>) -> Vec<WorktreeDeleteResult> {
    removals
        .into_iter()
        .map(|removal| {
            remove_merged_worktree(
                Path::new(&removal.repository_path),
                Path::new(&removal.worktree_path),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        collect_merged_worktrees, find_default_branch, is_ancestor, parse_worktree_porcelain,
        remove_merged_worktree, scan_for_merged_worktrees_in,
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

        assert_eq!(find_default_branch(repo.path()).as_deref(), Some("main"));
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

        assert_eq!(
            find_default_branch(repo.path()).as_deref(),
            Some("origin/trunk")
        );
    }

    #[test]
    fn falls_back_to_local_main_when_origin_head_is_dangling() {
        let repo = TestRepo::new();
        repo.git(&[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/missing",
        ]);

        assert_eq!(find_default_branch(repo.path()).as_deref(), Some("main"));
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
}
