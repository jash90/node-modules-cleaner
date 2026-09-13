// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod dev_caches;
mod fs_size;
mod git_worktrees;
mod settings;
mod tray;

use commands::{delete_folders, get_folder_size, scan_for_node_modules};
use dev_caches::{clean_dev_caches, scan_for_dev_caches};
use git_worktrees::{delete_merged_worktrees, scan_for_merged_worktrees};
use settings::{
    add_watched_folder, dock_toggle_available, get_settings, remove_watched_folder, set_hide_dock,
    SettingsStore,
};
use tauri::{Manager, RunEvent, WindowEvent};
use tray::{refresh_tray_now, tick_diagnostics};

fn main() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            let handle = app.handle();

            // Read before the webview exists. Applying the Dock setting any later means the icon
            // visibly appears and then disappears on every launch.
            let store = SettingsStore::load(settings::settings_file(handle));
            let current = store.snapshot();
            app.manage(store);
            settings::apply_dock_visibility(handle, current.hide_dock);

            tray::init(handle)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window puts the app in the menu bar rather than ending it — otherwise
            // hiding the Dock icon would leave no way to keep the app around at all.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            scan_for_node_modules,
            scan_for_merged_worktrees,
            delete_folders,
            delete_merged_worktrees,
            get_folder_size,
            scan_for_dev_caches,
            clean_dev_caches,
            get_settings,
            set_hide_dock,
            dock_toggle_available,
            add_watched_folder,
            remove_watched_folder,
            refresh_tray_now,
            tick_diagnostics
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app, event| match event {
        // Bringing the window forward has to wait for the event loop: doing it during `setup`
        // while the app is switching to the Accessory policy leaves the window minimized.
        RunEvent::Ready => {
            let hidden = app
                .try_state::<SettingsStore>()
                .map(|store| store.snapshot().hide_dock)
                .unwrap_or(false);
            if hidden {
                tray::show_main_window(app);
            }
        }

        // Closing the window only hides it, so the Dock icon would otherwise be inert and the
        // tray the only way back in. This is the gesture people actually reach for.
        #[cfg(target_os = "macos")]
        RunEvent::Reopen {
            has_visible_windows: false,
            ..
        } => tray::show_main_window(app),

        // `code: None` is the user closing the last window; `Some` is our own `app.exit(0)` from
        // the tray's Quit. Preventing both would leave a process nothing could stop.
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),

        _ => {}
    });
}
