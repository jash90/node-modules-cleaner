//! Directory size measurement that reports what deleting a tree actually frees.
//!
//! Three numbers instead of one, because they disagree in practice:
//!
//! - `logical` — sum of `metadata.len()`, what the old implementation returned. A sparse
//!   VM image or an evicted iCloud placeholder reports its nominal size here while
//!   occupying almost nothing on disk.
//! - `allocated` — sum of allocated blocks. What the tree really costs today.
//! - `reclaimable` — how much `allocated` comes back if the tree is deleted. A file
//!   hardlinked from outside the scanned tree (every `node_modules` under pnpm or bun)
//!   keeps its blocks alive after the delete, so it contributes zero.

use std::collections::HashMap;
use std::path::Path;
use walkdir::WalkDir;

use rayon::prelude::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirSize {
    pub logical: u64,
    pub allocated: u64,
    pub reclaimable: u64,
}

/// A file with more than one hardlink, tracked so it is counted once per tree.
#[derive(Debug, Clone, Copy)]
struct SharedFile {
    logical: u64,
    allocated: u64,
    /// Total hardlinks the filesystem reports for this inode.
    link_count: u64,
    /// How many of those links were found inside the scanned tree.
    links_seen: u64,
}

#[derive(Debug, Default)]
struct SizeAcc {
    /// Files with exactly one link — they cannot repeat inside the tree, so they are
    /// summed directly and are always fully reclaimable.
    logical: u64,
    allocated: u64,
    reclaimable: u64,
    /// Only multi-link files need identity tracking. Keeping them out of the common path
    /// matters: a scan of a large projects folder walks millions of single-link files.
    shared: HashMap<(u64, u64), SharedFile>,
}

impl SizeAcc {
    fn add(&mut self, file: FileFacts) {
        if file.link_count <= 1 {
            self.logical += file.logical;
            self.allocated += file.allocated;
            self.reclaimable += file.allocated;
            return;
        }

        self.shared
            .entry(file.identity)
            .and_modify(|entry| entry.links_seen += 1)
            .or_insert(SharedFile {
                logical: file.logical,
                allocated: file.allocated,
                link_count: file.link_count,
                links_seen: 1,
            });
    }

    fn merge(mut self, other: SizeAcc) -> SizeAcc {
        self.logical += other.logical;
        self.allocated += other.allocated;
        self.reclaimable += other.reclaimable;

        for (identity, entry) in other.shared {
            self.shared
                .entry(identity)
                .and_modify(|existing| existing.links_seen += entry.links_seen)
                .or_insert(entry);
        }

        self
    }

    fn finish(self) -> DirSize {
        let mut size = DirSize {
            logical: self.logical,
            allocated: self.allocated,
            reclaimable: self.reclaimable,
        };

        for entry in self.shared.values() {
            // Counted once, however many links point at it from inside the tree.
            size.logical += entry.logical;
            size.allocated += entry.allocated;

            // Blocks come back only when the tree holds every link to the inode.
            // Fewer links inside than the filesystem reports means something outside
            // — a pnpm store, another project — keeps the data alive.
            if entry.links_seen >= entry.link_count {
                size.reclaimable += entry.allocated;
            }
        }

        size
    }
}

#[derive(Debug, Clone, Copy)]
struct FileFacts {
    logical: u64,
    allocated: u64,
    link_count: u64,
    identity: (u64, u64),
}

#[cfg(unix)]
fn file_facts(metadata: &std::fs::Metadata) -> FileFacts {
    use std::os::unix::fs::MetadataExt;

    FileFacts {
        logical: metadata.len(),
        // `blocks()` is always in 512-byte units regardless of the filesystem block size.
        allocated: metadata.blocks() * 512,
        link_count: metadata.nlink(),
        identity: (metadata.dev(), metadata.ino()),
    }
}

#[cfg(windows)]
fn file_facts(metadata: &std::fs::Metadata) -> FileFacts {
    // Windows exposes neither allocation size nor inode identity through std::fs::Metadata.
    // Reporting the logical size keeps the numbers honest rather than invented; NTFS
    // compression and hardlinks are rare enough in a node_modules tree to accept this.
    FileFacts {
        logical: metadata.len(),
        allocated: metadata.len(),
        link_count: 1,
        identity: (0, 0),
    }
}

/// Size and age of a tree, from one traversal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DirScan {
    pub size: DirSize,
    pub last_modified: Option<i64>,
}

/// How many entries to gather before stat-ing them in parallel.
///
/// `readdir` is sequential no matter what, but the `lstat` per entry is where the time goes and
/// it parallelises freely. Batching keeps the walk's memory flat — a projects folder holds
/// millions of entries and holding them all would cost more than the scan saves — while giving
/// rayon chunks big enough to be worth scheduling.
const STAT_CHUNK: usize = 4096;

/// Measure a directory tree without crossing filesystem boundaries.
///
/// Staying on one filesystem matters on macOS: simulator runtimes mount their own APFS
/// volumes under `/Library/Developer/CoreSimulator/Volumes`, and a scan that follows them
/// bills a foreign volume's contents to the folder being measured.
///
/// Size and modification time come out of the same pass. They used to be two full walks of the
/// same tree, one per number, which on a large folder meant reading several million directory
/// entries twice over to answer two questions about the same files.
pub fn scan_dir(path: &Path) -> DirScan {
    let mut walker = WalkDir::new(path).same_file_system(true).into_iter();
    let mut totals = SizeAcc::default();
    let mut newest: Option<i64> = None;
    let mut batch: Vec<walkdir::DirEntry> = Vec::with_capacity(STAT_CHUNK);

    loop {
        batch.clear();
        // Counted separately from `batch.len()`: a batch of nothing but unreadable entries is
        // still progress, and treating it as the end of the walk would truncate the scan.
        let mut yielded = 0usize;
        for entry in walker.by_ref() {
            yielded += 1;
            if let Ok(entry) = entry {
                batch.push(entry);
            }
            if yielded == STAT_CHUNK {
                break;
            }
        }
        if yielded == 0 {
            break;
        }

        let (batch_totals, batch_newest) = batch
            .par_iter()
            .filter_map(|entry| {
                entry
                    .metadata()
                    .ok()
                    .map(|metadata| (entry.file_type().is_file(), metadata))
            })
            .fold(
                || (SizeAcc::default(), None::<i64>),
                |(mut totals, newest), (is_file, metadata)| {
                    if is_file {
                        totals.add(file_facts(&metadata));
                    }
                    // Directories count towards age but not towards size: a tree whose files
                    // are old but whose folders were touched yesterday is not fresh.
                    let seen = newer(newest, modified_unix(&metadata));
                    (totals, seen)
                },
            )
            .reduce(
                || (SizeAcc::default(), None::<i64>),
                |(totals, left), (other, right)| (totals.merge(other), newer(left, right)),
            );

        totals = totals.merge(batch_totals);
        newest = newer(newest, batch_newest);
    }

    DirScan {
        size: totals.finish(),
        last_modified: newest,
    }
}

/// Size only, for the callers that never ask about age.
pub fn measure_dir(path: &Path) -> DirSize {
    scan_dir(path).size
}

fn newer(left: Option<i64>, right: Option<i64>) -> Option<i64> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (left, right) => left.or(right),
    }
}

pub(crate) fn modified_unix(metadata: &std::fs::Metadata) -> Option<i64> {
    use std::time::UNIX_EPOCH;

    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|elapsed| elapsed.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
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

    #[test]
    fn plain_file_counts_once_and_is_fully_reclaimable() {
        let root = temp_root("plain");
        let _cleanup = TestDirectory(root.clone());
        fs::write(root.join("file.bin"), vec![7u8; 64 * 1024]).expect("fixture write");

        let size = measure_dir(&root);

        assert_eq!(size.logical, 64 * 1024);
        assert!(size.allocated >= 64 * 1024, "blocks cover the written bytes");
        assert_eq!(size.reclaimable, size.allocated);
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_inside_the_tree_are_counted_once_and_stay_reclaimable() {
        let root = temp_root("hardlink-inside");
        let _cleanup = TestDirectory(root.clone());
        let original = root.join("original.bin");
        fs::write(&original, vec![3u8; 128 * 1024]).expect("fixture write");
        fs::hard_link(&original, root.join("copy.bin")).expect("hardlink");

        let size = measure_dir(&root);

        // Two directory entries, one inode: the bytes exist once.
        assert_eq!(size.logical, 128 * 1024);
        // Both links live inside the tree, so deleting it releases the blocks.
        assert_eq!(size.reclaimable, size.allocated);
    }

    #[cfg(unix)]
    #[test]
    fn hardlink_reaching_outside_the_tree_reclaims_nothing() {
        let root = temp_root("hardlink-outside");
        let _cleanup = TestDirectory(root.clone());
        let outside = root.join("keeper.bin");
        let scanned = root.join("scanned");
        fs::create_dir_all(&scanned).expect("fixture dir");
        fs::write(&outside, vec![5u8; 96 * 1024]).expect("fixture write");
        fs::hard_link(&outside, scanned.join("linked.bin")).expect("hardlink");

        let size = measure_dir(&scanned);

        // This is the pnpm/bun case: node_modules looks big, deleting it frees nothing.
        assert_eq!(size.logical, 96 * 1024);
        assert!(size.allocated > 0);
        assert_eq!(size.reclaimable, 0);
    }

    #[cfg(unix)]
    #[test]
    fn sparse_file_allocates_far_less_than_its_logical_size() {
        use std::io::{Seek, SeekFrom, Write};

        let root = temp_root("sparse");
        let _cleanup = TestDirectory(root.clone());
        let path = root.join("disk.img");

        let mut file = fs::File::create(&path).expect("fixture create");
        file.seek(SeekFrom::Start(256 * 1024 * 1024))
            .expect("seek past the hole");
        file.write_all(b"tail").expect("fixture write");
        file.sync_all().expect("flush");
        drop(file);

        let size = measure_dir(&root);

        assert!(size.logical > 256 * 1024 * 1024);
        assert!(
            size.allocated < size.logical / 4,
            "sparse hole must not be billed: logical={} allocated={}",
            size.logical,
            size.allocated
        );
    }

    /// Cross-check against `du` on a real directory.
    ///
    /// Ignored by default because it needs a path from the machine it runs on:
    /// `MEASURE_PATH=~/.npm cargo test measure_against_du -- --ignored --nocapture`
    ///
    /// `du -sk` reports allocated blocks and already counts a hardlinked inode once, so
    /// its number should land on `allocated`, not `logical`.
    #[test]
    #[ignore]
    fn measure_against_du() {
        let Some(target) = std::env::var_os("MEASURE_PATH") else {
            eprintln!("set MEASURE_PATH to run this check");
            return;
        };
        let path = PathBuf::from(target);

        let measured = measure_dir(&path);

        let du = std::process::Command::new("du")
            .args(["-xsk", &path.to_string_lossy()])
            .output()
            .expect("du should run");
        let du_kb: u64 = String::from_utf8_lossy(&du.stdout)
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok())
            .expect("du should report a size");
        let du_bytes = du_kb * 1024;

        eprintln!(
            "{}\n  logical     {:>14}\n  allocated   {:>14}\n  reclaimable {:>14}\n  du -xsk     {:>14}",
            path.display(),
            measured.logical,
            measured.allocated,
            measured.reclaimable,
            du_bytes,
        );

        let drift = (measured.allocated as i128 - du_bytes as i128).unsigned_abs();
        let tolerance = (du_bytes / 50).max(1024 * 1024); // 2%, floor of 1 MiB
        assert!(
            drift <= tolerance as u128,
            "allocated={} strayed from du={} by {drift} bytes (tolerance {tolerance})",
            measured.allocated,
            du_bytes,
        );
    }

    #[test]
    fn last_modified_reports_the_newest_entry() {
        let root = temp_root("mtime");
        let _cleanup = TestDirectory(root.clone());
        fs::write(root.join("a.txt"), b"a").expect("fixture write");

        let stamp = scan_dir(&root).last_modified.expect("tree has files");
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_secs() as i64;

        assert!((now - stamp).abs() < 60, "stamp should be recent: {stamp}");
    }
}
