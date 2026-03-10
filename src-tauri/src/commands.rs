use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TopPackage {
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeModulesFolder {
    pub path: String,
    pub size: u64,
    pub parent_project: String,
    pub package_manager: String,
    pub top_packages: Vec<TopPackage>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScanResult {
    pub folders: Vec<NodeModulesFolder>,
    pub total_size: u64,
    pub scan_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DeleteResult {
    pub success: bool,
    pub path: String,
    pub error: Option<String>,
}

/// Calculate the size of a directory recursively
fn calculate_dir_size(path: &Path) -> u64 {
    let size = AtomicU64::new(0);

    WalkDir::new(path)
        .into_iter()
        .par_bridge()
        .filter_map(|e| e.ok())
        .for_each(|entry| {
            if entry.file_type().is_file() {
                if let Ok(metadata) = entry.metadata() {
                    size.fetch_add(metadata.len(), Ordering::Relaxed);
                }
            }
        });

    size.load(Ordering::Relaxed)
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

#[tauri::command]
pub async fn scan_for_node_modules(path: String) -> Result<ScanResult, String> {
    let scan_path = Path::new(&path);

    if !scan_path.exists() {
        return Err("Path does not exist".to_string());
    }

    if !scan_path.is_dir() {
        return Err("Path is not a directory".to_string());
    }

    // First, find all node_modules directories
    let node_modules_paths: Vec<_> = WalkDir::new(scan_path)
        .into_iter()
        .filter_entry(|e| {
            // Don't recurse into node_modules directories we find
            let parent_is_node_modules = e.path()
                .parent()
                .and_then(|p| p.file_name())
                .map(|n| n == "node_modules")
                .unwrap_or(false);

            // Skip hidden directories except the entry itself
            let is_hidden = e.file_name()
                .to_str()
                .map(|s| s.starts_with('.') && s != ".")
                .unwrap_or(false);

            !parent_is_node_modules && !is_hidden
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_dir() && e.file_name() == "node_modules")
        .map(|e| e.path().to_path_buf())
        .collect();

    // Calculate sizes in parallel
    let folders: Vec<NodeModulesFolder> = node_modules_paths
        .par_iter()
        .map(|path| {
            let size = calculate_dir_size(path);
            let parent = path.parent().unwrap_or(path);
            let top_packages = detect_top_packages(path);
            NodeModulesFolder {
                path: path.to_string_lossy().to_string(),
                size,
                parent_project: get_parent_project(path),
                package_manager: detect_package_manager(parent),
                top_packages,
            }
        })
        .collect();

    let total_size: u64 = folders.iter().map(|f| f.size).sum();

    Ok(ScanResult {
        folders,
        total_size,
        scan_path: path,
    })
}

#[tauri::command]
pub async fn delete_folders(paths: Vec<String>) -> Vec<DeleteResult> {
    paths
        .into_par_iter()
        .map(|path| {
            let path_ref = Path::new(&path);
            match fs::remove_dir_all(path_ref) {
                Ok(_) => DeleteResult {
                    success: true,
                    path,
                    error: None,
                },
                Err(e) => DeleteResult {
                    success: false,
                    path,
                    error: Some(e.to_string()),
                },
            }
        })
        .collect()
}

#[tauri::command]
pub async fn get_folder_size(path: String) -> Result<u64, String> {
    let path_ref = Path::new(&path);

    if !path_ref.exists() {
        return Err("Path does not exist".to_string());
    }

    Ok(calculate_dir_size(path_ref))
}
