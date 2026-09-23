use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::fs_size::{measure_dir, scan_dir, DirSize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopPackage {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeModulesFolder {
    pub path: String,
    /// Nominal size — the sum of file lengths. Kept as `size` for compatibility, but it
    /// overstates sparse files and iCloud placeholders. Show `reclaimable_size` instead
    /// when telling the user what they get back.
    pub size: u64,
    /// Blocks actually occupied on disk today.
    pub allocated_size: u64,
    /// Blocks that come back when this folder is deleted. Under pnpm and bun most files
    /// are hardlinks into a shared store, so this can be a fraction of `size`.
    pub reclaimable_size: u64,
    pub last_modified: Option<i64>,
    pub parent_project: String,
    pub package_manager: String,
    pub top_packages: Vec<TopPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub folders: Vec<NodeModulesFolder>,
    pub total_size: u64,
    pub total_allocated_size: u64,
    pub total_reclaimable_size: u64,
    pub scan_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteResult {
    pub success: bool,
    pub path: String,
    /// Bytes actually released. A partially failed delete still frees what it managed to
    /// remove, and the user deserves that number rather than a bare failure.
    pub removed_bytes: u64,
    pub error: Option<String>,
    /// Individual paths that survived, with the reason. Populated when a delete is
    /// partial — most often files owned by another user after a `sudo` install.
    pub failed_paths: Vec<FailedPath>,
    /// Set when the failures look like an ownership problem, carrying a command the user
    /// can run themselves.
    pub sudo_hint: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct FailedPath {
    pub path: String,
    pub reason: String,
}

/// Calculate the size of a directory recursively.
///
/// Returns the logical size only; callers that need allocation or reclaimable figures
/// should use [`measure_dir`] directly.
pub(crate) fn calculate_dir_size(path: &Path) -> u64 {
    measure_dir(path).logical
}

/// Full measurement of a directory: logical, allocated and reclaimable bytes.
pub(crate) fn measure_dir_size(path: &Path) -> DirSize {
    measure_dir(path)
}

/// Detect the package manager used in the parent directory of a node_modules folder
fn detect_package_manager(parent: &Path) -> String {
    if parent.join("bun.lockb").exists() || parent.join("bun.lock").exists() {
        "bun".to_string()
    } else if parent.join("pnpm-lock.yaml").exists() {
        "pnpm".to_string()
    } else if parent.join("yarn.lock").exists() {
        "yarn".to_string()
    } else if parent.join("package-lock.json").exists() {
        "npm".to_string()
    } else {
        "unknown".to_string()
    }
}

/// Known technology packages: (dependency name in package.json, display name)
const KNOWN_TECH: &[(&str, &str)] = &[
    ("react", "react"),
    ("react-native", "react-native"),
    ("next", "next"),
    ("expo", "expo"),
    ("express", "express"),
    ("hono", "hono"),
    ("@nestjs/core", "@nestjs"),
    ("vue", "vue"),
    ("@angular/core", "@angular"),
    ("svelte", "svelte"),
    ("nuxt", "nuxt"),
    ("gatsby", "gatsby"),
    ("@remix-run/react", "remix"),
    ("astro", "astro"),
    ("vite", "vite"),
    ("webpack", "webpack"),
    ("typescript", "typescript"),
    ("tailwindcss", "tailwindcss"),
    ("nx", "nx"),
    ("turbo", "turbo"),
    ("esbuild", "esbuild"),
    ("lerna", "lerna"),
];

/// Collect dependency names from a single package.json file
fn collect_dep_names(package_json_path: &Path) -> Vec<String> {
    let content = match fs::read_to_string(package_json_path) {
        Ok(c) => c,
        Err(_) => return Vec::new(),
    };

    let parsed: serde_json::Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut dep_names = Vec::new();
    for key in &["dependencies", "devDependencies"] {
        if let Some(obj) = parsed.get(key).and_then(|v| v.as_object()) {
            for name in obj.keys() {
                dep_names.push(name.clone());
            }
        }
    }
    dep_names
}

/// Check if a directory is a monorepo root (NX, Turbo, pnpm workspaces, yarn workspaces)
fn is_monorepo_root(parent: &Path) -> bool {
    if parent.join("nx.json").exists() {
        return true;
    }
    if parent.join("turbo.json").exists() {
        return true;
    }
    if parent.join("pnpm-workspace.yaml").exists() {
        return true;
    }
    // Check for "workspaces" field in package.json
    let pkg_path = parent.join("package.json");
    if let Ok(content) = fs::read_to_string(&pkg_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if parsed.get("workspaces").is_some() {
                return true;
            }
        }
    }
    false
}

/// Expand a workspace glob pattern (e.g. "apps/*") into package.json paths
fn expand_workspace_glob(parent: &Path, pattern: &str) -> Vec<PathBuf> {
    let trimmed = pattern
        .trim_end_matches('/')
        .trim_end_matches('*')
        .trim_end_matches('/');
    let base = parent.join(trimmed);
    if !base.is_dir() {
        return vec![];
    }

    if pattern.ends_with('*') || pattern.ends_with("/*") {
        // Glob: iterate child directories
        fs::read_dir(&base)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().join("package.json").exists())
            .map(|e| e.path().join("package.json"))
            .collect()
    } else {
        // Exact path
        let pkg = base.join("package.json");
        if pkg.exists() {
            vec![pkg]
        } else {
            vec![]
        }
    }
}

/// Find all workspace package.json files in a monorepo
fn find_workspace_package_jsons(parent: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();

    // Try to parse "workspaces" from root package.json
    let pkg_path = parent.join("package.json");
    let mut workspace_globs: Vec<String> = Vec::new();

    if let Ok(content) = fs::read_to_string(&pkg_path) {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) {
            if let Some(workspaces) = parsed.get("workspaces") {
                // Format: ["apps/*", "packages/*"]
                if let Some(arr) = workspaces.as_array() {
                    for item in arr {
                        if let Some(s) = item.as_str() {
                            workspace_globs.push(s.to_string());
                        }
                    }
                }
                // Format: { "packages": ["apps/*", "packages/*"] }
                if let Some(obj) = workspaces.as_object() {
                    if let Some(pkgs) = obj.get("packages").and_then(|v| v.as_array()) {
                        for item in pkgs {
                            if let Some(s) = item.as_str() {
                                workspace_globs.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    if !workspace_globs.is_empty() {
        for glob in &workspace_globs {
            results.extend(expand_workspace_glob(parent, glob));
        }
    } else {
        // Fallback: scan conventional directories (NX/Turbo without workspaces field)
        for dir_name in &["apps", "libs", "packages"] {
            let dir = parent.join(dir_name);
            if dir.is_dir() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let pkg = entry.path().join("package.json");
                        if pkg.exists() {
                            results.push(pkg);
                        }
                    }
                }
            }
        }
    }

    results
}

/// Detect top 5 known technology packages by reading package.json (monorepo-aware)
fn detect_top_packages(node_modules_path: &Path) -> Vec<TopPackage> {
    let parent = match node_modules_path.parent() {
        Some(p) => p,
        None => return Vec::new(),
    };

    // Collect deps from root package.json
    let mut all_deps = collect_dep_names(&parent.join("package.json"));

    // If monorepo — also collect from workspace packages
    if is_monorepo_root(parent) {
        for pkg_json in find_workspace_package_jsons(parent) {
            all_deps.extend(collect_dep_names(&pkg_json));
        }
    }

    // Deduplicate
    all_deps.sort();
    all_deps.dedup();

    // Match against KNOWN_TECH (priority from array order)
    let mut packages = Vec::new();
    for &(dep_name, display_name) in KNOWN_TECH {
        if all_deps.iter().any(|d| d == dep_name) {
            packages.push(TopPackage {
                name: display_name.to_string(),
            });
        }
    }

    packages.truncate(5);
    packages
}

/// Get the parent project name from a node_modules path
fn get_parent_project(node_modules_path: &Path) -> String {
    node_modules_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("Unknown")
        .to_string()
}

/// Dot-directories that hold whole checkouts rather than a tool's own state, so the scan
/// walks into them: `git worktree` homes and the agent worktrees Claude Code keeps.
///
/// Every other dot-directory below the root is skipped. Walking them all was tried, and on a
/// real `~/Projects` it offered CMake mirrors under `.cxx`, `.next/standalone` and bundled
/// `.bun`/`.pnpm` outputs, and a `.vscode-test` app bundle for deletion — build products whose
/// `node_modules` is not a project's install. The root itself is never skipped, so a user who
/// picks `~/.config` still gets it scanned.
const WALKED_DOT_DIRS: &[&str] = &[".worktrees", ".claude"];

/// Plainly named folders that are huge and never hold a project's own `node_modules`.
const SKIPPED_DIRS: &[&str] = &["Library", "Pods", "CloudStorage"];

fn is_skipped_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if name.starts_with('.') {
        return !WALKED_DOT_DIRS.contains(&name);
    }
    // A macOS app bundle ships its own `node_modules` as part of the app; deleting it breaks
    // the app rather than freeing a reinstallable dependency tree.
    if name.ends_with(".app") || SKIPPED_DIRS.contains(&name) {
        return true;
    }
    // A Rust build directory is huge and never a JS project, but `target` is a common enough
    // name that it is pruned only where Cargo put it.
    name == "target"
        && path
            .parent()
            .is_some_and(|parent| parent.join("Cargo.toml").exists())
}

fn find_node_modules_paths(scan_path: &Path) -> Vec<PathBuf> {
    if scan_path
        .file_name()
        .is_some_and(|name| name == "node_modules")
    {
        // `symlink_metadata`, not `is_dir`: the walk below never offers a linked
        // `node_modules`, and `delete_folders` refuses one, so the root must not either.
        let is_real_dir =
            fs::symlink_metadata(scan_path).is_ok_and(|meta| meta.file_type().is_dir());
        return if is_real_dir {
            vec![scan_path.to_path_buf()]
        } else {
            Vec::new()
        };
    }

    // One walk per top-level folder, in parallel: a projects folder is many independent
    // trees, and a single walker spends most of its time waiting on one directory read at a
    // time.
    let Ok(children) = fs::read_dir(scan_path) else {
        return Vec::new();
    };
    let root_device = device_of(scan_path);
    let subtrees: Vec<PathBuf> = children
        .filter_map(Result::ok)
        // `file_type` does not follow symlinks, so linked folders stay out as they always have.
        .filter(|child| child.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|child| child.path())
        .filter(|path| !is_skipped_dir(path))
        // Staying on one filesystem keeps the scan out of mounted volumes and network shares,
        // which are slow to walk and not what the user asked to clean.
        .filter(|path| device_of(path) == root_device)
        .collect();

    let mut paths: Vec<PathBuf> = subtrees
        .par_iter()
        .flat_map_iter(|subtree| find_in_subtree(subtree))
        .collect();
    paths.sort();
    paths
}

fn find_in_subtree(subtree: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut entries = WalkDir::new(subtree).same_file_system(true).into_iter();

    while let Some(entry) = entries.next() {
        let Ok(entry) = entry else {
            continue;
        };

        // The `is_dir` guard matters: `skip_current_dir` on a file skips the rest of its
        // parent, so a stray file named `.cache` would hide its sibling projects.
        if !entry.file_type().is_dir() {
            continue;
        }

        // The subtree's own top was already checked against the skip rules by the caller.
        if entry.depth() > 0 && is_skipped_dir(entry.path()) {
            entries.skip_current_dir();
            continue;
        }

        if entry.file_name() == "node_modules" {
            paths.push(entry.path().to_path_buf());
            entries.skip_current_dir();
        }
    }

    paths
}

#[cfg(unix)]
fn device_of(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| metadata.dev())
}

/// Windows has no cheap device id through std; mounted volumes there get their own drive
/// letter, which the per-subtree `same_file_system` walk still refuses to cross.
#[cfg(not(unix))]
fn device_of(_path: &Path) -> Option<u64> {
    None
}

#[tauri::command]
pub async fn scan_for_node_modules(path: String) -> Result<ScanResult, String> {
    // The walk and the size measurement are blocking filesystem work; running them on the
    // async runtime's worker would stall every other command while a large folder is scanned.
    tauri::async_runtime::spawn_blocking(move || scan_for_node_modules_blocking(path))
        .await
        .map_err(|error| format!("Scan did not finish: {error}"))?
}

fn scan_for_node_modules_blocking(path: String) -> Result<ScanResult, String> {
    let scan_path = Path::new(&path);

    if !scan_path.exists() {
        return Err("Path does not exist".to_string());
    }

    if !scan_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    // Keep only outermost node_modules directories. Their recursive size
    // already includes any dependency-level node_modules folders inside.
    let node_modules_paths = find_node_modules_paths(scan_path);

    // Calculate sizes in parallel
    let folders: Vec<NodeModulesFolder> = node_modules_paths
        .par_iter()
        .map(|path| {
            let scan = scan_dir(path);
            let size = scan.size;
            let parent = path.parent().unwrap_or(path);
            let top_packages = detect_top_packages(path);
            NodeModulesFolder {
                path: path.to_string_lossy().to_string(),
                size: size.logical,
                allocated_size: size.allocated,
                reclaimable_size: size.reclaimable,
                last_modified: scan.last_modified,
                parent_project: get_parent_project(path),
                package_manager: detect_package_manager(parent),
                top_packages,
            }
        })
        .collect();

    let total_size: u64 = folders.iter().map(|f| f.size).sum();
    let total_allocated_size: u64 = folders.iter().map(|f| f.allocated_size).sum();
    let total_reclaimable_size: u64 = folders.iter().map(|f| f.reclaimable_size).sum();

    Ok(ScanResult {
        folders,
        total_size,
        total_allocated_size,
        total_reclaimable_size,
        scan_path: path,
    })
}

/// True when the path exists and belongs to a different user than the running process.
///
/// A `sudo npm install` or `sudo bun install` leaves root-owned files behind; the user
/// then hits `Permission denied` with no explanation of why their own cache resists them.
#[cfg(unix)]
fn is_owned_by_other_user(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    fs::symlink_metadata(path)
        .map(|metadata| metadata.uid() != unsafe { libc::getuid() })
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_owned_by_other_user(_path: &Path) -> bool {
    false
}

/// Delete a tree, continuing past individual failures instead of aborting on the first.
///
/// `fs::remove_dir_all` stops at the first error and reports only that error, which reads
/// as a total failure even when almost everything was removed. Walking the tree ourselves
/// lets us remove what we can and report precisely what survived.
pub(crate) fn remove_tree_collecting(root: &Path) -> Vec<FailedPath> {
    let mut failures = Vec::new();

    // The overwhelmingly common case is that nothing is in the way, and `remove_dir_all` does
    // it in one call rather than a `contents_first` walk that stats every entry on the way to
    // unlinking it. The walk below is what produces the per-path report, so it is kept for
    // exactly the case that needs it: something refused to go.
    //
    // This does not reduce filesystem events — every unlink is still one event either way — it
    // removes the traversal that was paying for a report nobody needed.
    if fs::remove_dir_all(root).is_ok() {
        return failures;
    }

    // A root that is not there — never was, or went away mid-delete — is not a failure: the
    // folder is gone either way, the same stance worktree removal takes on a worktree removed
    // elsewhere. Reporting it as a failure left the row on screen asking the user to check
    // permissions on something that no longer exists.
    if matches!(fs::symlink_metadata(root), Err(error) if error.kind() == std::io::ErrorKind::NotFound)
    {
        return failures;
    }

    // contents_first so files and nested directories go before their parents.
    for entry in WalkDir::new(root).contents_first(true).into_iter() {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(FailedPath {
                    path: error
                        .path()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|| root.to_string_lossy().to_string()),
                    reason: error.to_string(),
                });
                continue;
            }
        };

        let entry_path = entry.path();
        let outcome = if entry.file_type().is_dir() {
            fs::remove_dir(entry_path)
        } else {
            fs::remove_file(entry_path)
        };

        if let Err(error) = outcome {
            failures.push(FailedPath {
                path: entry_path.to_string_lossy().to_string(),
                reason: error.to_string(),
            });
        }
    }

    failures
}

#[tauri::command]
pub async fn delete_folders(paths: Vec<String>) -> Vec<DeleteResult> {
    let requested = paths.clone();
    match tauri::async_runtime::spawn_blocking(move || delete_folders_blocking(paths)).await {
        Ok(results) => results,
        // Nothing is known about how far the delete got, so every path is reported as
        // failed rather than silently dropped from the result.
        Err(error) => requested
            .into_iter()
            .map(|path| rejected(path, format!("Delete did not finish: {error}")))
            .collect(),
    }
}

/// Why a path must not be deleted, or `None` when it is a real `node_modules` directory.
///
/// The frontend only ever sends paths the scan found, but this command removes whatever it is
/// given, so it checks for itself: a wrong path here is an unrecoverable `rm -rf`. A symlink is
/// refused because the tree it points at belongs to someone else. A path that does not exist
/// passes — there is nothing to remove, and [`remove_tree_collecting`] reports it as done.
fn delete_target_rejection(path: &Path) -> Option<String> {
    if !path.is_absolute() {
        return Some("Refusing to delete a relative path".to_string());
    }
    if path.file_name().is_none_or(|name| name != "node_modules") {
        return Some("Refusing to delete a folder that is not named node_modules".to_string());
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Some(
            "Refusing to delete a symlink; the folder it points at is not this tool's to remove"
                .to_string(),
        ),
        Ok(metadata) if !metadata.is_dir() => {
            Some("Refusing to delete something that is not a directory".to_string())
        }
        Ok(_) => None,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => Some(error.to_string()),
    }
}

fn rejected(path: String, reason: String) -> DeleteResult {
    DeleteResult {
        success: false,
        removed_bytes: 0,
        error: Some(reason.clone()),
        failed_paths: vec![FailedPath {
            path: path.clone(),
            reason,
        }],
        sudo_hint: None,
        path,
    }
}

fn delete_folders_blocking(paths: Vec<String>) -> Vec<DeleteResult> {
    paths
        .into_par_iter()
        .map(|path| {
            let path_ref = Path::new(&path);

            // Checked before measuring too: a rejected path must not even be walked.
            if let Some(reason) = delete_target_rejection(path_ref) {
                return rejected(path, reason);
            }

            // Measure before deleting: afterwards there is nothing left to measure, and
            // reporting "freed 0 bytes" for a successful delete would be worse than useless.
            let before = measure_dir_size(path_ref);
            let failures = remove_tree_collecting(path_ref);
            // A clean delete leaves nothing behind, so the second full walk is only paid for
            // when something survived and the freed figure has to subtract it.
            let leftover = if failures.is_empty() {
                0
            } else {
                measure_dir_size(path_ref).reclaimable
            };

            let removed_bytes = before.reclaimable.saturating_sub(leftover);
            let foreign_owner = failures
                .iter()
                .any(|failure| is_owned_by_other_user(Path::new(&failure.path)));

            DeleteResult {
                success: failures.is_empty(),
                error: failures.first().map(|failure| failure.reason.clone()),
                sudo_hint: foreign_owner.then(|| format!("sudo rm -rf {}", shell_quote(&path))),
                path,
                removed_bytes,
                failed_paths: failures,
            }
        })
        .collect()
}

/// Single-quote a path for a shell command shown to the user.
fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', r"'\''"))
}

#[tauri::command]
pub async fn get_folder_size(path: String) -> Result<u64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let path_ref = Path::new(&path);

        if !path_ref.exists() {
            return Err("Path does not exist".to_string());
        }

        Ok(calculate_dir_size(path_ref))
    })
    .await
    .map_err(|error| format!("Size check did not finish: {error}"))?
}

#[cfg(test)]
mod tests {
    use super::{delete_folders_blocking, find_node_modules_paths, remove_tree_collecting};
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn temp_root(label: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-{label}-{}-{unique}",
            std::process::id(),
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");
        root
    }

    fn delete_one(path: &Path) -> super::DeleteResult {
        delete_folders_blocking(vec![path.to_string_lossy().to_string()])
            .pop()
            .expect("one path in, one result out")
    }

    #[test]
    fn scan_includes_a_hidden_scan_root() {
        let root = temp_root("hidden-root");
        let _cleanup = TestDirectory(root.clone());
        let hidden = root.join(".config");
        let project = hidden.join("project/node_modules");
        fs::create_dir_all(&project).expect("fixture should be created");

        assert_eq!(find_node_modules_paths(&hidden), vec![project]);
    }

    /// `delete_folders` refuses symlinks, so offering one would be a row that can only fail.
    #[cfg(unix)]
    #[test]
    fn scan_does_not_offer_a_symlinked_node_modules_picked_as_the_root() {
        let root = temp_root("symlink-root");
        let _cleanup = TestDirectory(root.clone());
        let real = root.join("store");
        fs::create_dir_all(&real).expect("fixture should be created");
        let link = root.join("node_modules");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        assert!(find_node_modules_paths(&link).is_empty());
    }

    #[test]
    fn scan_finds_projects_inside_worktree_homes() {
        let root = temp_root("worktree-homes");
        let _cleanup = TestDirectory(root.clone());
        let plain = root.join("repo/.worktrees/feature/node_modules");
        let agent = root.join("repo/.claude/worktrees/task/node_modules");
        fs::create_dir_all(&plain).expect("fixture should be created");
        fs::create_dir_all(&agent).expect("fixture should be created");

        let mut found = find_node_modules_paths(&root);
        found.sort();

        assert_eq!(found, vec![agent, plain]);
    }

    /// Every one of these was offered for deletion on a real machine once dot-directories were
    /// walked: build mirrors, bundled outputs and an app bundle whose own code lives there.
    #[test]
    fn scan_skips_build_output_and_app_bundles() {
        let root = temp_root("build-output");
        let _cleanup = TestDirectory(root.clone());
        for junk in [
            "app/android/app/.cxx/Debug/CMakeFiles/x.dir/Users/me/app/node_modules",
            "web/.next/standalone/node_modules",
            "web/dist/assets/__node_modules/.bun/pkg/node_modules",
            "ios/build/Products/.pnpm/pkg/node_modules",
            "ext/.vscode-test/Code.app/Contents/Resources/app/node_modules",
            "tools/Editor.app/Contents/Resources/app/node_modules",
            "tool/.opencode/node_modules",
        ] {
            fs::create_dir_all(root.join(junk)).expect("fixture should be created");
        }
        let kept = root.join("web/node_modules");
        fs::create_dir_all(&kept).expect("fixture should be created");

        assert_eq!(find_node_modules_paths(&root), vec![kept]);
    }

    #[test]
    fn scan_still_skips_listed_directories() {
        let root = temp_root("skip-list");
        let _cleanup = TestDirectory(root.clone());
        for skipped in [".git", ".npm", ".cache", "Library", "Pods"] {
            fs::create_dir_all(root.join(skipped).join("pkg/node_modules"))
                .expect("fixture should be created");
        }
        let kept = root.join("app/node_modules");
        fs::create_dir_all(&kept).expect("fixture should be created");

        assert_eq!(find_node_modules_paths(&root), vec![kept]);
    }

    #[test]
    fn scan_skips_target_only_beside_cargo_toml() {
        let root = temp_root("target");
        let _cleanup = TestDirectory(root.clone());
        fs::create_dir_all(root.join("crate/target/x/node_modules"))
            .expect("fixture should be created");
        fs::write(root.join("crate/Cargo.toml"), "[package]").expect("fixture write");
        let js_target = root.join("js/target/node_modules");
        fs::create_dir_all(&js_target).expect("fixture should be created");

        assert_eq!(find_node_modules_paths(&root), vec![js_target]);
    }

    #[test]
    fn delete_rejects_a_path_not_named_node_modules() {
        let root = temp_root("reject-name");
        let _cleanup = TestDirectory(root.clone());
        let target = root.join("src");
        fs::create_dir_all(&target).expect("fixture should be created");

        let result = delete_one(&target);

        assert!(!result.success);
        assert_eq!(result.removed_bytes, 0);
        assert!(result.error.is_some());
        assert_eq!(result.failed_paths.len(), 1);
        assert!(target.exists(), "rejected folder must survive");
    }

    #[test]
    fn delete_rejects_a_relative_path() {
        let results = delete_folders_blocking(vec!["node_modules".to_string()]);

        assert!(!results[0].success);
        assert!(results[0].error.is_some());
        assert_eq!(results[0].failed_paths.len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn delete_rejects_a_symlink_named_node_modules() {
        let root = temp_root("reject-symlink");
        let _cleanup = TestDirectory(root.clone());
        let real = root.join("real");
        fs::create_dir_all(&real).expect("fixture should be created");
        fs::write(real.join("keep.txt"), b"keep").expect("fixture write");
        let link = root.join("node_modules");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        let result = delete_one(&link);

        assert!(!result.success);
        assert_eq!(result.removed_bytes, 0);
        assert!(
            real.join("keep.txt").exists(),
            "symlink target must survive"
        );
        assert!(link.exists(), "the symlink itself is left alone too");
    }

    #[test]
    fn delete_of_a_missing_node_modules_is_success() {
        let root = temp_root("missing");
        let _cleanup = TestDirectory(root.clone());

        let result = delete_one(&root.join("node_modules"));

        assert!(result.success, "gone either way: {:?}", result.error);
        assert_eq!(result.removed_bytes, 0);
        assert!(result.failed_paths.is_empty());
        assert!(result.error.is_none());
    }

    #[test]
    fn remove_tree_collecting_treats_a_missing_root_as_done() {
        let root = temp_root("missing-root");
        let _cleanup = TestDirectory(root.clone());

        assert!(remove_tree_collecting(&root.join("gone")).is_empty());
    }

    #[test]
    fn delete_removes_node_modules_and_reports_freed_bytes() {
        let root = temp_root("delete-ok");
        let _cleanup = TestDirectory(root.clone());
        let target = root.join("node_modules");
        fs::create_dir_all(target.join("pkg")).expect("fixture should be created");
        fs::write(target.join("pkg/index.js"), vec![1u8; 32 * 1024]).expect("fixture write");

        let result = delete_one(&target);

        assert!(result.success);
        assert!(result.removed_bytes >= 32 * 1024);
        assert!(!target.exists());
    }

    #[test]
    fn scan_keeps_only_outermost_node_modules_directories() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "node-modules-cleaner-nested-test-{}-{unique}",
            std::process::id(),
        ));
        let _cleanup = TestDirectory(root.clone());
        let outer = root.join("project/node_modules");
        let nested = outer.join("dependency/node_modules");
        let separate = root.join("other-project/node_modules");

        fs::create_dir_all(&nested).expect("nested fixture should be created");
        fs::create_dir_all(&separate).expect("separate fixture should be created");

        let mut found = find_node_modules_paths(&root);
        found.sort();

        assert_eq!(found, vec![separate, outer]);
    }
}
