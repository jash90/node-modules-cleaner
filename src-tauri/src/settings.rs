//! The first thing this app remembers between runs.
//!
//! Scan results, selections and sort order are deliberately ephemeral, but two things cannot be:
//! whether the Dock icon is hidden, and which folders the menu bar keeps an eye on. The Dock
//! setting in particular has to be readable *before* the webview exists — applying it later
//! means the icon visibly flashes into the Dock on every launch — which is why this is a real
//! file read in `setup()` rather than anything on the JavaScript side.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, Runtime, State};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// macOS only. Elsewhere it is stored but inert.
    pub hide_dock: bool,
    /// Folders the tray counts worktrees and free space for.
    pub watched_folders: Vec<String>,
}

/// The settings file plus the in-memory copy every command reads.
pub struct SettingsStore {
    file: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    pub fn load(file: PathBuf) -> Self {
        let current = Mutex::new(read_settings(&file));
        Self { file, current }
    }

    /// A clone, never a guard. Callers include the tray thread, whose tick can take tens of
    /// seconds of Git work — holding the lock across that would stall every settings command.
    pub fn snapshot(&self) -> Settings {
        self.current
            .lock()
            .map(|settings| settings.clone())
            .unwrap_or_default()
    }

    /// Apply a change, persist it, and hand back the new state.
    fn update(&self, change: impl FnOnce(&mut Settings)) -> Result<Settings, String> {
        let updated = {
            let mut guard = self
                .current
                .lock()
                .map_err(|_| "Settings lock was poisoned".to_string())?;
            change(&mut guard);
            guard.clone()
        };
        write_settings(&self.file, &updated)?;
        Ok(updated)
    }
}

pub fn settings_file<R: Runtime>(app: &AppHandle<R>) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}

/// Never fails: an absent file is a first run, and an unreadable or malformed one is not worth
/// blocking startup over — the defaults are harmless and the next write repairs the file.
pub fn read_settings(file: &Path) -> Settings {
    fs::read_to_string(file)
        .ok()
        .and_then(|contents| serde_json::from_str(&contents).ok())
        .unwrap_or_default()
}

pub fn write_settings(file: &Path, settings: &Settings) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Could not create the settings directory: {error}"))?;
    }
    let contents = serde_json::to_string_pretty(settings)
        .map_err(|error| format!("Could not serialize settings: {error}"))?;
    fs::write(file, contents).map_err(|error| format!("Could not write settings: {error}"))
}

/// Resolve a folder the way the rest of the app does, so the same directory reached by two
/// different paths (a symlink, a trailing slash) cannot be watched twice.
fn normalize_folder(folder: &str) -> String {
    Path::new(folder)
        .canonicalize()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|_| folder.trim_end_matches('/').to_string())
}

pub fn add_folder(settings: &mut Settings, folder: &str) {
    let normalized = normalize_folder(folder);
    if normalized.is_empty() || settings.watched_folders.contains(&normalized) {
        return;
    }
    settings.watched_folders.push(normalized);
}

pub fn remove_folder(settings: &mut Settings, folder: &str) {
    let normalized = normalize_folder(folder);
    settings
        .watched_folders
        .retain(|watched| watched != &normalized && watched != folder);
}

/// Whether this platform can hide the Dock icon at all. Tauri exposes `set_dock_visibility`
/// only on macOS, so everywhere else the UI shows the toggle as unavailable rather than
/// pretending it worked.
pub const fn dock_toggle_supported() -> bool {
    cfg!(target_os = "macos")
}

pub fn apply_dock_visibility<R: Runtime>(app: &AppHandle<R>, hide_dock: bool) {
    #[cfg(target_os = "macos")]
    {
        if let Err(error) = app.set_dock_visibility(!hide_dock) {
            eprintln!("Could not change Dock visibility: {error}");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, hide_dock);
    }
}

#[tauri::command]
pub async fn get_settings(store: State<'_, SettingsStore>) -> Result<Settings, String> {
    Ok(store.snapshot())
}

#[tauri::command]
pub async fn dock_toggle_available() -> bool {
    dock_toggle_supported()
}

#[tauri::command]
pub async fn set_hide_dock(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    hidden: bool,
) -> Result<Settings, String> {
    let updated = store.update(|settings| settings.hide_dock = hidden)?;
    apply_dock_visibility(&app, updated.hide_dock);
    Ok(updated)
}

#[tauri::command]
pub async fn add_watched_folder(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    folder: String,
) -> Result<Settings, String> {
    let updated = store.update(|settings| add_folder(settings, &folder))?;
    crate::tray::request_refresh(&app);
    Ok(updated)
}

#[tauri::command]
pub async fn remove_watched_folder(
    app: AppHandle,
    store: State<'_, SettingsStore>,
    folder: String,
) -> Result<Settings, String> {
    let updated = store.update(|settings| remove_folder(settings, &folder))?;
    crate::tray::request_refresh(&app);
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::{add_folder, read_settings, remove_folder, write_settings, Settings};
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_dir(label: &str) -> PathBuf {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "node-modules-cleaner-settings-{label}-{}-{id}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create fixture directory");
        path
    }

    #[test]
    fn a_missing_settings_file_reads_as_defaults() {
        let root = temp_dir("missing");

        let settings = read_settings(&root.join("settings.json"));

        assert_eq!(settings, Settings::default());
        assert!(!settings.hide_dock);
        assert!(settings.watched_folders.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupt_settings_file_reads_as_defaults_instead_of_failing() {
        let root = temp_dir("corrupt");
        let file = root.join("settings.json");
        fs::write(&file, "{ this is not json").expect("write fixture");

        assert_eq!(read_settings(&file), Settings::default());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unknown_and_missing_fields_do_not_break_an_older_settings_file() {
        let root = temp_dir("partial");
        let file = root.join("settings.json");
        fs::write(&file, r#"{"hide_dock": true, "from_a_future_version": 3}"#)
            .expect("write fixture");

        let settings = read_settings(&file);

        assert!(settings.hide_dock);
        assert!(settings.watched_folders.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn settings_round_trip_through_a_directory_that_does_not_exist_yet() {
        let root = temp_dir("round-trip");
        // The config directory is created on demand, like it would be on a first run.
        let file = root.join("nested").join("settings.json");
        let settings = Settings {
            hide_dock: true,
            watched_folders: vec!["/Users/someone/My Projects".to_string()],
        };

        write_settings(&file, &settings).expect("write settings");

        assert_eq!(read_settings(&file), settings);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn the_same_folder_is_never_watched_twice() {
        let root = temp_dir("dedupe");
        let folder = root.to_string_lossy().to_string();
        let mut settings = Settings::default();

        add_folder(&mut settings, &folder);
        add_folder(&mut settings, &folder);
        add_folder(&mut settings, &format!("{folder}/"));

        assert_eq!(settings.watched_folders.len(), 1);

        remove_folder(&mut settings, &folder);
        assert!(settings.watched_folders.is_empty());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn a_folder_that_no_longer_exists_can_still_be_removed() {
        let mut settings = Settings {
            hide_dock: false,
            watched_folders: vec!["/gone/for/good".to_string()],
        };

        remove_folder(&mut settings, "/gone/for/good");

        assert!(settings.watched_folders.is_empty());
    }
}
