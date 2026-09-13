//! The menu bar readout.
//!
//! Two numbers you would otherwise have to open the app to learn: how much room is left on the
//! volumes holding the watched folders, and how many worktrees are ready to be removed right now.
//!
//! Platform reality, which shapes the design: `TrayIcon::set_title` is unsupported on Windows and
//! conditional on Linux, so the title is a bonus and the *menu* carries the information everywhere.

use crate::git_worktrees::count_removable_worktrees;
use crate::settings::{Settings, SettingsStore};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::time::Duration;
use sysinfo::Disks;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, Runtime};

/// Counting worktrees shells out to Git once per repository, so this is paced for a background
/// task rather than for a live readout. Anything the user wants sooner comes through `Refresh now`.
const REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
const TRAY_ID: &str = "main";

/// Monochrome template image, embedded rather than bundled as a resource so there is no runtime
/// path to get wrong. macOS recolours it for light/dark and dims it on click.
const TRAY_ICON: &[u8] = include_bytes!("../icons/tray-template.png");

/// Handle the commands use to ask for an out-of-band refresh.
pub struct TrayRefresh(Sender<()>);

pub fn request_refresh<R: Runtime>(app: &AppHandle<R>) {
    if let Some(refresh) = app.try_state::<TrayRefresh>() {
        let _ = refresh.0.send(());
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VolumeSpace {
    pub name: String,
    pub mount_point: String,
    pub available: u64,
    pub total: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FolderStat {
    pub folder: String,
    pub removable_worktrees: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct TrayStats {
    pub volumes: Vec<VolumeSpace>,
    pub folders: Vec<FolderStat>,
    pub removable_worktrees: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Volume {
    name: String,
    mount_point: PathBuf,
    available: u64,
    total: u64,
}

/// Free space as Finder reports it, which is not what `df` prints.
///
/// sysinfo asks macOS for `VolumeAvailableCapacityForImportantUsage`, so on APFS the figure
/// accounts for purgeable space and lands a gigabyte or two away from `df`'s `statfs` view.
/// Finder's number is the one a person recognises, so a mismatch with `df` is expected here,
/// not a bug.
fn read_volumes() -> Vec<Volume> {
    Disks::new_with_refreshed_list()
        .list()
        .iter()
        .map(|disk| Volume {
            name: disk.name().to_string_lossy().to_string(),
            mount_point: disk.mount_point().to_path_buf(),
            available: disk.available_space(),
            total: disk.total_space(),
        })
        .collect()
}

/// The volume a folder actually lives on.
///
/// Every path is under `/`, so matching any mount point is not enough — the answer is the
/// *longest* mount point that is a prefix. On macOS `~/Projects` matches both `/` and
/// `/System/Volumes/Data`, and only the latter is the volume whose free space will change.
fn pick_volume<'a>(volumes: &'a [Volume], folder: &Path) -> Option<&'a Volume> {
    volumes
        .iter()
        .filter(|volume| folder.starts_with(&volume.mount_point))
        .max_by_key(|volume| volume.mount_point.as_os_str().len())
}

/// Same 1024-based scale and rounding as the window's `formatSize`, so the two never disagree
/// about what "412 GB" means.
fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }

    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// Shorten a home-relative path the way a shell would, so menu entries stay readable.
fn display_folder(folder: &str) -> String {
    let home = std::env::var("HOME").ok().filter(|home| !home.is_empty());
    match home {
        Some(home) if folder == home => "~".to_string(),
        Some(home) if folder.starts_with(&format!("{home}/")) => {
            format!("~{}", &folder[home.len()..])
        }
        _ => folder.to_string(),
    }
}

pub fn collect_stats(settings: &Settings) -> TrayStats {
    let volumes = read_volumes();
    let mut stats = TrayStats::default();

    for folder in &settings.watched_folders {
        let path = Path::new(folder);
        let removable = count_removable_worktrees(path);
        stats.removable_worktrees += removable;
        stats.folders.push(FolderStat {
            folder: folder.clone(),
            removable_worktrees: removable,
        });

        if let Some(volume) = pick_volume(&volumes, path) {
            let mount_point = volume.mount_point.to_string_lossy().to_string();
            // Several watched folders commonly share one volume; report it once.
            if !stats
                .volumes
                .iter()
                .any(|known| known.mount_point == mount_point)
            {
                stats.volumes.push(VolumeSpace {
                    name: volume.name.clone(),
                    mount_point,
                    available: volume.available,
                    total: volume.total,
                });
            }
        }
    }

    stats
}

/// The text beside the icon. `None` until there is something to say, so a fresh install shows a
/// bare icon instead of a meaningless zero.
fn tray_title(stats: &TrayStats) -> Option<String> {
    if stats.folders.is_empty() {
        return None;
    }
    // The tightest volume is the one worth putting in front of someone.
    let tightest = stats
        .volumes
        .iter()
        .min_by_key(|volume| volume.available)
        .map(|volume| format_bytes(volume.available));

    Some(match tightest {
        Some(free) => format!("{} · {free}", stats.removable_worktrees),
        None => stats.removable_worktrees.to_string(),
    })
}

fn tray_tooltip(stats: &TrayStats) -> String {
    if stats.folders.is_empty() {
        return "Node Modules Cleaner — no folders watched".to_string();
    }
    format!(
        "Node Modules Cleaner — {} worktree(s) ready to clean in {} folder(s)",
        stats.removable_worktrees,
        stats.folders.len()
    )
}

fn volume_label(volume: &VolumeSpace) -> String {
    let name = if volume.name.trim().is_empty() {
        volume.mount_point.clone()
    } else {
        volume.name.clone()
    };
    format!(
        "{name} — {} free of {}",
        format_bytes(volume.available),
        format_bytes(volume.total)
    )
}

fn folder_label(folder: &FolderStat) -> String {
    let count = folder.removable_worktrees;
    format!(
        "{} — {count} worktree{} ready to clean",
        display_folder(&folder.folder),
        if count == 1 { "" } else { "s" }
    )
}

/// Rebuilt from scratch on every refresh rather than mutating item handles: the number of
/// volumes and folders changes with the settings, so there is no fixed set of items to update.
fn build_menu<R: Runtime>(app: &AppHandle<R>, stats: &TrayStats) -> tauri::Result<Menu<R>> {
    let menu = Menu::new(app)?;
    menu.append(&MenuItem::with_id(
        app,
        "open",
        "Open Node Modules Cleaner",
        true,
        None::<&str>,
    )?)?;
    menu.append(&PredefinedMenuItem::separator(app)?)?;

    if stats.folders.is_empty() {
        menu.append(&MenuItem::with_id(
            app,
            "empty",
            "No folders watched — add one in Settings",
            false,
            None::<&str>,
        )?)?;
    } else {
        for (index, volume) in stats.volumes.iter().enumerate() {
            menu.append(&MenuItem::with_id(
                app,
                format!("volume-{index}"),
                volume_label(volume),
                false,
                None::<&str>,
            )?)?;
        }
        if !stats.volumes.is_empty() {
            menu.append(&PredefinedMenuItem::separator(app)?)?;
        }
        for (index, folder) in stats.folders.iter().enumerate() {
            menu.append(&MenuItem::with_id(
                app,
                format!("folder-{index}"),
                folder_label(folder),
                false,
                None::<&str>,
            )?)?;
        }
    }

    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItem::with_id(
        app,
        "refresh",
        "Refresh now",
        true,
        None::<&str>,
    )?)?;
    menu.append(&MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?)?;

    Ok(menu)
}

pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn apply<R: Runtime>(app: &AppHandle<R>, stats: &TrayStats) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    if let Ok(menu) = build_menu(app, stats) {
        let _ = tray.set_menu(Some(menu));
    }
    let _ = tray.set_tooltip(Some(tray_tooltip(stats)));
    let _ = tray.set_title(tray_title(stats).as_deref());
}

/// Build the tray and start the thread that keeps it current.
///
/// Deliberately returns `Ok(())` on failure to build: on Linux the tray needs
/// `libayatana-appindicator3` present at runtime, and a machine without it should still get a
/// working window rather than an app that refuses to start.
pub fn init(app: &AppHandle) -> tauri::Result<()> {
    let icon = tauri::image::Image::from_bytes(TRAY_ICON)?;
    let menu = build_menu(app, &TrayStats::default())?;

    let built = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        // With a menu attached the left click opens it, which is the macOS status-item norm —
        // so "Open" is a menu entry rather than a click handler that would never fire.
        .show_menu_on_left_click(true)
        .menu(&menu)
        .tooltip(tray_tooltip(&TrayStats::default()))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "refresh" => request_refresh(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app);

    if let Err(error) = built {
        eprintln!("Could not create the tray icon, continuing without it: {error}");
        return Ok(());
    }

    let (sender, receiver) = mpsc::channel();
    app.manage(TrayRefresh(sender));
    spawn_refresh_thread(app.clone(), receiver);
    Ok(())
}

fn spawn_refresh_thread(app: AppHandle, receiver: Receiver<()>) {
    std::thread::spawn(move || loop {
        // Clone the settings and drop the lock before the Git work: a tick can run for tens of
        // seconds, and every settings command would queue behind a held lock.
        let settings = app.state::<SettingsStore>().snapshot();
        let stats = collect_stats(&settings);
        apply(&app, &stats);

        match receiver.recv_timeout(REFRESH_INTERVAL) {
            Ok(()) => {
                // Several requests can pile up while a tick runs; they all mean the same thing.
                while receiver.try_recv().is_ok() {}
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    });
}

#[tauri::command]
pub async fn refresh_tray_now(app: AppHandle) {
    request_refresh(&app);
}

#[cfg(test)]
mod tests {
    use super::{
        folder_label, format_bytes, pick_volume, tray_title, FolderStat, TrayStats, Volume,
        VolumeSpace,
    };
    use std::path::{Path, PathBuf};

    fn volume(name: &str, mount_point: &str, available: u64) -> Volume {
        Volume {
            name: name.to_string(),
            mount_point: PathBuf::from(mount_point),
            available,
            total: available * 2,
        }
    }

    #[test]
    fn the_longest_matching_mount_point_wins() {
        // Every path matches "/", so a plain "starts_with" would always answer the root volume.
        let volumes = vec![
            volume("Macintosh HD", "/", 10),
            volume("Data", "/System/Volumes/Data", 20),
            volume("Backup", "/Volumes/Backup", 30),
        ];

        assert_eq!(
            pick_volume(&volumes, Path::new("/System/Volumes/Data/Users/x/Projects"))
                .map(|volume| volume.name.as_str()),
            Some("Data")
        );
        assert_eq!(
            pick_volume(&volumes, Path::new("/Volumes/Backup/old"))
                .map(|volume| volume.name.as_str()),
            Some("Backup")
        );
        assert_eq!(
            pick_volume(&volumes, Path::new("/etc")).map(|volume| volume.name.as_str()),
            Some("Macintosh HD")
        );
    }

    #[test]
    fn a_mount_point_only_matches_whole_path_components() {
        let volumes = vec![volume("Backup", "/Volumes/Backup", 30)];

        // "/Volumes/BackupOld" is a different volume, not a child of this one.
        assert!(pick_volume(&volumes, Path::new("/Volumes/BackupOld/x")).is_none());
    }

    #[test]
    fn sizes_use_the_same_scale_as_the_window() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.0 KB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.0 GB");
        assert_eq!(format_bytes(442 * 1024 * 1024 * 1024), "442.0 GB");
    }

    #[test]
    fn no_watched_folders_means_no_title_at_all() {
        // A bare "0" next to the icon on a fresh install would be noise, not information.
        assert_eq!(tray_title(&TrayStats::default()), None);
    }

    #[test]
    fn the_title_reports_the_tightest_volume() {
        let stats = TrayStats {
            volumes: vec![
                VolumeSpace {
                    name: "Roomy".to_string(),
                    mount_point: "/".to_string(),
                    available: 900 * 1024 * 1024 * 1024,
                    total: 1000 * 1024 * 1024 * 1024,
                },
                VolumeSpace {
                    name: "Tight".to_string(),
                    mount_point: "/Volumes/Tight".to_string(),
                    available: 3 * 1024 * 1024 * 1024,
                    total: 500 * 1024 * 1024 * 1024,
                },
            ],
            folders: vec![FolderStat {
                folder: "/Users/x/Projects".to_string(),
                removable_worktrees: 4,
            }],
            removable_worktrees: 4,
        };

        assert_eq!(tray_title(&stats).as_deref(), Some("4 · 3.0 GB"));
    }

    #[test]
    fn folder_labels_read_as_english() {
        assert!(folder_label(&FolderStat {
            folder: "/Users/x/Projects".to_string(),
            removable_worktrees: 1,
        })
        .ends_with("1 worktree ready to clean"));
        assert!(folder_label(&FolderStat {
            folder: "/Users/x/Projects".to_string(),
            removable_worktrees: 0,
        })
        .ends_with("0 worktrees ready to clean"));
    }
}
