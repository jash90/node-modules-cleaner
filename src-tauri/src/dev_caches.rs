//! Shared developer caches — the disk hogs that live outside any single project.
//!
//! `node_modules` folders are the visible waste; the larger, quieter waste sits in caches
//! shared across every project, in store directories abandoned when a tool changed its
//! default location, and in runtimes kept one copy per version forever.
//!
//! Two rules shape this module:
//!
//! 1. **Delegate where the tool knows better.** `uv cache prune` and `conda clean` skip
//!    entries still referenced by live environments; a blind `rm -rf` of the same path
//!    breaks them. Where an official prune exists, we run it instead of deleting.
//! 2. **Never delete a directory that also holds live state.** `~/Library/pnpm` is
//!    `PNPM_HOME`: its `store/` is disposable, its `bin/` holds the launcher on `PATH`.

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::fs_size::{last_modified_unix, measure_dir};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CacheKind {
    /// Package manager download/content caches. Always safe to drop; re-downloaded on demand.
    PackageManager,
    /// A store directory left behind when a tool moved its default location.
    OrphanedStore,
    /// One copy per version of a runtime or browser build.
    Runtime,
    /// A log file that nothing rotates.
    Log,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Safety {
    /// Pure cache. Removing it costs download time, nothing else.
    Safe,
    /// Removing it is fine but has a consequence worth reading first.
    NeedsReview,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CleanupMethod {
    /// Delete the directory tree.
    RemoveDir,
    /// Truncate a file in place, keeping its inode.
    ///
    /// Deleting a log a running process holds open does not free anything: the process
    /// keeps writing to the now-invisible inode and the space returns only after a
    /// restart. Truncating releases the blocks immediately.
    TruncateFile,
    /// Hand the job to the tool that owns the cache.
    ExternalCommand {
        program: String,
        args: Vec<String>,
        /// Human-readable form of the command, for showing before it runs.
        display: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheTarget {
    pub id: String,
    pub kind: CacheKind,
    pub label: String,
    pub path: String,
    pub logical_size: u64,
    pub allocated_size: u64,
    pub reclaimable_size: u64,
    pub last_modified: Option<i64>,
    pub safety: Safety,
    pub cleanup: CleanupMethod,
    pub note: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CacheScanResult {
    pub targets: Vec<CacheTarget>,
    pub total_reclaimable_size: u64,
    /// Things worth telling the user that are not targets — e.g. pnpm not being installed,
    /// so orphaned stores could not be told apart from the active one.
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct CacheCleanRequest {
    pub id: String,
    pub path: String,
    /// Carried back from the scan result, so the frontend never has to decide how a
    /// particular cache should be cleaned.
    pub cleanup: CleanupMethod,
}

#[derive(Debug, Serialize)]
pub struct CacheCleanResult {
    pub id: String,
    pub path: String,
    pub success: bool,
    pub removed_bytes: u64,
    pub error: Option<String>,
    /// Output from an external prune command, which usually reports what it freed.
    pub output: Option<String>,
}

// ---------------------------------------------------------------------------
// Target descriptions (paths only — sizes are measured later, in parallel)
// ---------------------------------------------------------------------------

struct Candidate {
    id: &'static str,
    kind: CacheKind,
    label: String,
    path: PathBuf,
    safety: Safety,
    cleanup: CleanupMethod,
    note: Option<String>,
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

fn external(program: &str, args: &[&str]) -> CleanupMethod {
    CleanupMethod::ExternalCommand {
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        display: format!("{program} {}", args.join(" ")),
    }
}

/// Package manager caches that can simply be deleted.
fn package_manager_caches(home: &Path) -> Vec<Candidate> {
    let plain: &[(&'static str, &str, &str)] = &[
        (
            "npm-cacache",
            ".npm/_cacache",
            "npm content cache — rebuilt on next install",
        ),
        (
            "npm-npx",
            ".npm/_npx",
            "one-off packages fetched by npx; nothing depends on them",
        ),
        (
            "bun-install-cache",
            ".bun/install/cache",
            "bun package cache — re-downloaded on next install",
        ),
        (
            "yarn-cache",
            ".yarn/cache",
            "yarn Berry cache — re-downloaded on next install",
        ),
        (
            "yarn-cache-legacy",
            "Library/Caches/Yarn",
            "yarn Classic cache — re-downloaded on next install",
        ),
    ];

    plain
        .iter()
        .map(|(id, relative, note)| Candidate {
            id,
            kind: CacheKind::PackageManager,
            label: relative.to_string(),
            path: home.join(relative),
            safety: Safety::Safe,
            cleanup: CleanupMethod::RemoveDir,
            note: Some(note.to_string()),
        })
        .collect()
}

/// Every pnpm store on the machine, with the active one told apart from the leftovers.
///
/// pnpm has changed its default store location more than once. A machine that has used
/// pnpm for years accumulates copies at each old path, and nothing ever cleans them up.
fn pnpm_stores(home: &Path, warnings: &mut Vec<String>) -> Vec<Candidate> {
    let active = active_pnpm_store();
    if active.is_none() {
        warnings.push(
            "pnpm not found on PATH — its stores are reported as 'needs review' because the \
             active one cannot be identified."
                .to_string(),
        );
    }

    // Known store locations across pnpm versions. `~/Library/pnpm` is PNPM_HOME on macOS:
    // only its `store` subdirectory is disposable, never the directory itself.
    let known = [
        home.join(".local/share/pnpm/store"),
        home.join(".pnpm-store"),
        home.join("Library/pnpm/store"),
        home.join("AppData/Local/pnpm/store"),
    ];

    known
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| {
            let is_active = active
                .as_ref()
                .map(|active| active.starts_with(&path))
                .unwrap_or(false);

            if is_active {
                Candidate {
                    id: "pnpm-store-active",
                    kind: CacheKind::PackageManager,
                    label: format!("{} (active store)", display_path(&path, home)),
                    path,
                    safety: Safety::Safe,
                    cleanup: external("pnpm", &["store", "prune"]),
                    note: Some(
                        "Active store — pruned rather than deleted, so packages still \
                         referenced by installed projects survive."
                            .to_string(),
                    ),
                }
            } else {
                Candidate {
                    id: "pnpm-store-orphaned",
                    kind: CacheKind::OrphanedStore,
                    label: format!("{} (abandoned store)", display_path(&path, home)),
                    path,
                    safety: if active.is_some() {
                        Safety::Safe
                    } else {
                        Safety::NeedsReview
                    },
                    note: Some(
                        "Left behind after pnpm moved its default store location. Deleting a \
                         store never breaks an installed project: files in node_modules are \
                         hardlinks, so the data survives as long as a link points at it."
                            .to_string(),
                    ),
                    cleanup: CleanupMethod::RemoveDir,
                }
            }
        })
        .collect()
}

fn active_pnpm_store() -> Option<PathBuf> {
    let output = Command::new("pnpm").args(["store", "path"]).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

/// Caches owned by a tool that ships its own prune command.
fn prunable_caches(home: &Path) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    let uv = home.join(".cache/uv");
    if uv.exists() {
        candidates.push(Candidate {
            id: "uv-cache",
            kind: CacheKind::PackageManager,
            label: ".cache/uv".to_string(),
            path: uv,
            safety: Safety::Safe,
            cleanup: external("uv", &["cache", "prune"]),
            note: Some(
                "Pruned by uv itself, which keeps entries still referenced by environments."
                    .to_string(),
            ),
        });
    }

    // Gradle keeps compiled scripts and transforms under caches/<version> and downloaded
    // dependencies under caches/modules-2. The former rebuilds locally in seconds; the
    // latter would have to come back over the network, so it is left alone.
    let gradle_caches = home.join(".gradle/caches");
    if gradle_caches.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&gradle_caches) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let looks_like_version = name
                    .chars()
                    .next()
                    .map(|c| c.is_ascii_digit())
                    .unwrap_or(false);

                if looks_like_version && entry.path().is_dir() {
                    candidates.push(Candidate {
                        id: "gradle-version-cache",
                        kind: CacheKind::PackageManager,
                        label: format!(".gradle/caches/{name}"),
                        path: entry.path(),
                        safety: Safety::Safe,
                        cleanup: CleanupMethod::RemoveDir,
                        note: Some(
                            "Compiled build scripts and transforms for one Gradle version; \
                             rebuilt locally. Downloaded dependencies in caches/modules-2 \
                             are kept."
                                .to_string(),
                        ),
                    });
                }
            }
        }
    }

    candidates
}

/// Runtimes and browser builds kept one directory per version.
fn versioned_runtimes(home: &Path) -> Vec<Candidate> {
    let mut candidates = Vec::new();

    // Puppeteer downloads a full Chrome per version and never removes the old ones.
    for channel in ["chrome", "chrome-headless-shell"] {
        let root = home.join(".cache/puppeteer").join(channel);
        for path in older_versions(&root) {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            candidates.push(Candidate {
                id: "puppeteer-old-build",
                kind: CacheKind::Runtime,
                label: format!(".cache/puppeteer/{channel}/{name}"),
                path: path.clone(),
                safety: Safety::Safe,
                cleanup: CleanupMethod::RemoveDir,
                note: Some("Superseded browser build; the newest one is kept.".to_string()),
            });
        }
    }

    // nvm keeps every Node version ever installed, each with its own global packages.
    for nvm_root in [
        home.join(".nvm/versions/node"),
        home.join(".config/nvm/versions/node"),
    ] {
        let active = active_node_version();
        for path in older_versions(&nvm_root) {
            let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
            if active.as_deref() == Some(name.as_str()) {
                continue;
            }

            candidates.push(Candidate {
                id: "nvm-old-node",
                kind: CacheKind::Runtime,
                label: format!("nvm {name}"),
                path: path.clone(),
                safety: Safety::NeedsReview,
                cleanup: CleanupMethod::RemoveDir,
                note: Some(format!(
                    "Node {name} and its globally installed packages. Reinstall with \
                     `nvm install {name}` if needed — globals do not come back with it."
                )),
            });
        }
    }

    candidates
}

fn active_node_version() -> Option<String> {
    let output = Command::new("node").arg("-v").output().ok()?;
    let version = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!version.is_empty()).then_some(version)
}

/// Numeric components of a version directory name, for ordering.
///
/// Lexical ordering is wrong here and dangerously so: `"v8.0.0" > "v25.9.0"` as strings,
/// which would mark the newest Node as stale and keep an ancient one instead. Comparing
/// the numbers in order — `[8, 0, 0]` against `[25, 9, 0]` — gets it right, and works
/// equally well for Chrome builds like `mac_arm-148.0.7778.167`.
fn version_key(name: &str) -> Vec<u64> {
    name.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

/// Every direct child of `root` except the newest one.
fn older_versions(root: &Path) -> Vec<PathBuf> {
    if !root.is_dir() {
        return Vec::new();
    }

    let mut versions: Vec<PathBuf> = match std::fs::read_dir(root) {
        Ok(entries) => entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect(),
        Err(_) => return Vec::new(),
    };

    versions.sort_by_key(|path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        (version_key(&name), name)
    });
    versions.pop(); // keep the newest
    versions
}

/// Log files nothing rotates.
///
/// Homebrew services write to a single file forever. Postgres in particular appends the
/// full text of every failing query, so one bad import loop can add gigabytes.
fn unrotated_logs(_home: &Path) -> Vec<Candidate> {
    /// Below this a log is not worth showing.
    const MIN_INTERESTING_BYTES: u64 = 256 * 1024 * 1024;

    let mut candidates = Vec::new();

    for log_dir in ["/opt/homebrew/var/log", "/usr/local/var/log"] {
        let Ok(entries) = std::fs::read_dir(log_dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("log") {
                continue;
            }

            let big_enough = entry
                .metadata()
                .map(|m| m.len() >= MIN_INTERESTING_BYTES)
                .unwrap_or(false);
            if !big_enough {
                continue;
            }

            let name = path.file_name().unwrap_or_default().to_string_lossy();
            candidates.push(Candidate {
                id: "unrotated-log",
                kind: CacheKind::Log,
                label: format!("{log_dir}/{name}"),
                path: path.clone(),
                safety: Safety::Safe,
                cleanup: CleanupMethod::TruncateFile,
                note: Some(
                    "Emptied in place rather than deleted — the service holds this file open, \
                     and removing it would keep the space locked until a restart."
                        .to_string(),
                ),
            });
        }
    }

    candidates
}

fn display_path(path: &Path, home: &Path) -> String {
    path.strip_prefix(home)
        .map(|rest| format!("~/{}", rest.display()))
        .unwrap_or_else(|_| path.display().to_string())
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn scan_for_dev_caches() -> Result<CacheScanResult, String> {
    collect_targets()
}

/// Synchronous core of the scan, kept separate so it can be exercised without a Tauri runtime.
pub(crate) fn collect_targets() -> Result<CacheScanResult, String> {
    let home = home_dir().ok_or_else(|| "Could not determine the home directory".to_string())?;
    let mut warnings = Vec::new();

    let mut candidates = Vec::new();
    candidates.extend(package_manager_caches(&home));
    candidates.extend(pnpm_stores(&home, &mut warnings));
    candidates.extend(prunable_caches(&home));
    candidates.extend(versioned_runtimes(&home));
    candidates.extend(unrotated_logs(&home));

    candidates.retain(|candidate| candidate.path.exists());

    // Below this a target is noise in the list rather than a saving worth a click.
    const MIN_WORTH_SHOWING: u64 = 64 * 1024 * 1024;

    // Measuring is the slow part — every candidate walks a tree, so do them together.
    let mut targets: Vec<CacheTarget> = candidates
        .into_par_iter()
        .map(|candidate| {
            let size = if candidate.path.is_file() {
                // A single file needs no walk; its own metadata is the answer.
                std::fs::metadata(&candidate.path)
                    .map(|metadata| file_size_of(&metadata))
                    .unwrap_or_default()
            } else {
                measure_dir(&candidate.path)
            };

            // A prune keeps whatever is still referenced, so the directory's current size
            // is an upper bound on the saving, not the saving itself. Claiming the whole
            // 18 GB of an active pnpm store when the prune will free a fraction of it
            // would be the same overstatement this module exists to remove.
            let delegated = matches!(candidate.cleanup, CleanupMethod::ExternalCommand { .. });

            CacheTarget {
                id: candidate.id.to_string(),
                kind: candidate.kind,
                label: candidate.label,
                path: candidate.path.to_string_lossy().to_string(),
                logical_size: size.logical,
                allocated_size: size.allocated,
                reclaimable_size: if delegated { 0 } else { size.reclaimable },
                last_modified: last_modified_unix(&candidate.path),
                safety: candidate.safety,
                cleanup: candidate.cleanup,
                note: candidate.note,
            }
        })
        .collect();

    // Filter and order on what the target occupies; a delegated prune reports zero
    // reclaimable by design and would otherwise drop off the list entirely.
    targets.retain(|target| target.allocated_size >= MIN_WORTH_SHOWING);
    targets.sort_by_key(|target| std::cmp::Reverse(target.allocated_size));

    // Only figures we can stand behind. Prune targets contribute nothing here, so the
    // headline number is a floor rather than a promise.
    let total_reclaimable_size = targets.iter().map(|t| t.reclaimable_size).sum();

    Ok(CacheScanResult {
        targets,
        total_reclaimable_size,
        warnings,
    })
}

fn file_size_of(metadata: &std::fs::Metadata) -> crate::fs_size::DirSize {
    #[cfg(unix)]
    let allocated = {
        use std::os::unix::fs::MetadataExt;
        metadata.blocks() * 512
    };
    #[cfg(not(unix))]
    let allocated = metadata.len();

    crate::fs_size::DirSize {
        logical: metadata.len(),
        allocated,
        reclaimable: allocated,
    }
}

#[tauri::command]
pub async fn clean_dev_caches(targets: Vec<CacheCleanRequest>) -> Vec<CacheCleanResult> {
    targets.into_par_iter().map(clean_one).collect()
}

fn clean_one(request: CacheCleanRequest) -> CacheCleanResult {
    let path = PathBuf::from(&request.path);
    let method = request.cleanup.clone();
    let before = if path.is_file() {
        std::fs::metadata(&path)
            .map(|m| file_size_of(&m).reclaimable)
            .unwrap_or(0)
    } else {
        measure_dir(&path).reclaimable
    };

    match method {
        CleanupMethod::TruncateFile => match std::fs::File::create(&path) {
            // File::create truncates without unlinking, so the writing process keeps its
            // descriptor valid and the blocks are released immediately.
            Ok(_) => CacheCleanResult {
                id: request.id,
                path: request.path,
                success: true,
                removed_bytes: before,
                error: None,
                output: None,
            },
            Err(error) => failure(request, before, error.to_string()),
        },

        CleanupMethod::RemoveDir => {
            let failures = crate::commands::remove_tree_collecting(&path);
            let leftover = if path.exists() {
                measure_dir(&path).reclaimable
            } else {
                0
            };

            CacheCleanResult {
                id: request.id,
                path: request.path,
                success: failures.is_empty(),
                removed_bytes: before.saturating_sub(leftover),
                error: failures.first().map(|failure| {
                    format!("{} ({} paths could not be removed)", failure.reason, failures.len())
                }),
                output: None,
            }
        }

        CleanupMethod::ExternalCommand { program, args, .. } => {
            match Command::new(&program).args(&args).output() {
                Ok(output) => {
                    let after = measure_dir(&path).reclaimable;
                    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    CacheCleanResult {
                        id: request.id,
                        path: request.path,
                        success: output.status.success(),
                        removed_bytes: before.saturating_sub(after),
                        error: (!output.status.success())
                            .then(|| String::from_utf8_lossy(&output.stderr).trim().to_string()),
                        output: (!text.is_empty()).then_some(text),
                    }
                }
                Err(error) => failure(
                    request,
                    0,
                    format!("could not run `{program}`: {error}"),
                ),
            }
        }
    }
}

fn failure(request: CacheCleanRequest, removed: u64, error: String) -> CacheCleanResult {
    CacheCleanResult {
        id: request.id,
        path: request.path,
        success: false,
        removed_bytes: removed,
        error: Some(error),
        output: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
            "node-modules-cleaner-caches-{label}-{}-{unique}",
            std::process::id(),
        ));
        fs::create_dir_all(&root).expect("fixture root should be created");
        root
    }

    /// Print what the scan finds on the machine it runs on.
    ///
    /// `cargo test report_real_caches -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn report_real_caches() {
        let result = collect_targets().expect("scan should succeed");

        for warning in &result.warnings {
            eprintln!("warning: {warning}");
        }
        for target in &result.targets {
            eprintln!(
                "{:>14}  {:?}  {}  [{}]",
                result_size(target.reclaimable_size),
                target.kind,
                target.label,
                match &target.cleanup {
                    CleanupMethod::ExternalCommand { display, .. } => display.clone(),
                    CleanupMethod::TruncateFile => "truncate".to_string(),
                    CleanupMethod::RemoveDir => "remove".to_string(),
                }
            );
        }
        eprintln!(
            "total reclaimable: {}",
            result_size(result.total_reclaimable_size)
        );
    }

    fn result_size(bytes: u64) -> String {
        format!("{:.2} GB", bytes as f64 / 1_073_741_824.0)
    }

    #[test]
    fn older_versions_keeps_the_newest_directory() {
        let root = temp_root("versions");
        let _cleanup = TestDirectory(root.clone());
        for version in ["mac_arm-143.0.7499.42", "mac_arm-148.0.7778.167"] {
            fs::create_dir_all(root.join(version)).expect("fixture dir");
        }

        let old = older_versions(&root);

        assert_eq!(old.len(), 1);
        assert!(old[0].ends_with("mac_arm-143.0.7499.42"));
    }

    #[test]
    fn newest_version_is_chosen_numerically_not_lexically() {
        let root = temp_root("semver");
        let _cleanup = TestDirectory(root.clone());
        // Lexically "v8.0.0" sorts above both of the others, so a string sort would keep
        // the oldest Node and offer the newest for deletion.
        for version in ["v8.0.0", "v10.0.0", "v22.19.0"] {
            fs::create_dir_all(root.join(version)).expect("fixture dir");
        }

        let old = older_versions(&root);
        let names: Vec<String> = old
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(names, vec!["v8.0.0", "v10.0.0"]);
    }

    #[test]
    fn older_versions_is_empty_for_a_single_version() {
        let root = temp_root("single");
        let _cleanup = TestDirectory(root.clone());
        fs::create_dir_all(root.join("v22.19.0")).expect("fixture dir");

        assert!(older_versions(&root).is_empty());
    }

    #[test]
    fn pnpm_home_bin_is_never_a_target() {
        let home = temp_root("pnpm-home");
        let _cleanup = TestDirectory(home.clone());
        fs::create_dir_all(home.join("Library/pnpm/store/v10")).expect("fixture dir");
        fs::create_dir_all(home.join("Library/pnpm/bin")).expect("fixture dir");

        let mut warnings = Vec::new();
        let candidates = pnpm_stores(&home, &mut warnings);

        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.path.ends_with("store")),
            "only the store subdirectory may be targeted, never PNPM_HOME itself"
        );
    }

    #[test]
    fn truncating_a_log_keeps_the_inode() {
        let root = temp_root("log");
        let _cleanup = TestDirectory(root.clone());
        let log = root.join("service.log");
        fs::write(&log, vec![b'x'; 4096]).expect("fixture write");

        #[cfg(unix)]
        let inode_before = {
            use std::os::unix::fs::MetadataExt;
            fs::metadata(&log).expect("metadata").ino()
        };

        let result = clean_one(CacheCleanRequest {
            id: "unrotated-log".to_string(),
            path: log.to_string_lossy().to_string(),
            cleanup: CleanupMethod::TruncateFile,
        });

        assert!(result.success);
        assert_eq!(fs::metadata(&log).expect("still exists").len(), 0);

        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let inode_after = fs::metadata(&log).expect("metadata").ino();
            assert_eq!(
                inode_before, inode_after,
                "truncation must preserve the inode so an open writer keeps working"
            );
        }
    }
}
