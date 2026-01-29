// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use commands::{delete_folders, get_folder_size, scan_for_node_modules};

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            scan_for_node_modules,
            delete_folders,
            get_folder_size
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
