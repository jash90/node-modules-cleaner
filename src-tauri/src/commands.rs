use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::fs_size::{last_modified_unix, measure_dir, DirSize};

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

fn find_node_modules_paths(scan_path: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let mut entries = WalkDir::new(scan_path).into_iter();

    while let Some(entry) = entries.next() {
        let Ok(entry) = entry else {
            continue;
        };

        let is_hidden = entry
            .file_name()
            .to_str()
            .map(|name| name.starts_with('.') && name != ".")
            .unwrap_or(false);

        if is_hidden && entry.file_type().is_dir() {
            entries.skip_current_dir();
            continue;
        }

        if entry.file_type().is_dir() && entry.file_name() == "node_modules" {
            paths.push(entry.path().to_path_buf());
            entries.skip_current_dir();
        }
    }

    paths
}

#[tauri::command]
pub async fn scan_for_node_modules(path: String) -> Result<ScanResult, String> {
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
            let size = measure_dir_size(path);
            let parent = path.parent().unwrap_or(path);
            let top_packages = detect_top_packages(path);
            NodeModulesFolder {
                path: path.to_string_lossy().to_string(),
                size: size.logical,
                allocated_size: size.allocated,
                reclaimable_size: size.reclaimable,
                last_modified: last_modified_unix(path),
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
    paths
        .into_par_iter()
        .map(|path| {
            let path_ref = Path::new(&path);

            // Measure before deleting: afterwards there is nothing left to measure, and
            // reporting "freed 0 bytes" for a successful delete would be worse than useless.
            let before = measure_dir_size(path_ref);
            let failures = remove_tree_collecting(path_ref);
            let leftover = if path_ref.exists() {
                measure_dir_size(path_ref).reclaimable
            } else {
                0
            };

            let removed_bytes = before.reclaimable.saturating_sub(leftover);
            let foreign_owner = failures
                .iter()
                .any(|failure| is_owned_by_other_user(Path::new(&failure.path)));

            DeleteResult {
                success: failures.is_empty(),
                error: failures.first().map(|failure| failure.reason.clone()),
                sudo_hint: foreign_owner
                    .then(|| format!("sudo rm -rf {}", shell_quote(&path))),
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
    let path_ref = Path::new(&path);

    if !path_ref.exists() {
        return Err("Path does not exist".to_string());
    }

    Ok(calculate_dir_size(path_ref))
}

#[cfg(test)]
mod tests {
    use super::find_node_modules_paths;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TestDirectory(PathBuf);

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
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
