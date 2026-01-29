use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use walkdir::WalkDir;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct NodeModulesFolder {
    pub path: String,
    pub size: u64,
    pub parent_project: String,
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
            NodeModulesFolder {
                path: path.to_string_lossy().to_string(),
                size,
                parent_project: get_parent_project(path),
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
