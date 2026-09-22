export interface TopPackage {
  name: string;
}

export interface NodeModulesFolder {
  path: string;
  /** Nominal size. Overstates sparse files and iCloud placeholders — show `reclaimable_size` instead. */
  size: number;
  /** Blocks the folder actually occupies today. */
  allocated_size: number;
  /** Blocks that come back on delete. Far below `size` when pnpm/bun hardlink into a shared store. */
  reclaimable_size: number;
  last_modified: number | null;
  parent_project: string;
  package_manager: string;
  top_packages: TopPackage[];
}

export interface ScanResult {
  folders: NodeModulesFolder[];
  total_size: number;
  total_allocated_size: number;
  total_reclaimable_size: number;
  scan_path: string;
}

export interface FailedPath {
  path: string;
  reason: string;
}

export interface DeleteResult {
  success: boolean;
  path: string;
  /** Bytes actually released — a partial delete still frees what it managed to remove. */
  removed_bytes: number;
  error: string | null;
  failed_paths: FailedPath[];
  /** Present when failures look like an ownership problem; a command the user can run. */
  sudo_hint: string | null;
}

export type CacheKind = 'package_manager' | 'orphaned_store' | 'runtime' | 'log';
export type Safety = 'safe' | 'needs_review';

export type CleanupMethod =
  | { type: 'remove_dir' }
  | { type: 'truncate_file' }
  | { type: 'external_command'; program: string; args: string[]; display: string };

export interface CacheTarget {
  id: string;
  kind: CacheKind;
  label: string;
  path: string;
  logical_size: number;
  allocated_size: number;
  reclaimable_size: number;
  last_modified: number | null;
  safety: Safety;
  cleanup: CleanupMethod;
  note: string | null;
}

export interface CacheScanResult {
  targets: CacheTarget[];
  total_reclaimable_size: number;
  warnings: string[];
}

export interface CacheCleanResult {
  id: string;
  path: string;
  success: boolean;
  removed_bytes: number;
  error: string | null;
  /** Output from an external prune command, which usually reports what it freed. */
  output: string | null;
}

/** How a worktree relates to its repository's base branches. */
export type WorktreeState = 'merged' | 'squashed' | 'stale';

export interface MergedWorktree {
  path: string;
  branch: string;
  repository_path: string;
  repository_name: string;
  /** Empty for a stale entry, which was never compared with anything. */
  base_branch: string;
  size: number;
  is_dirty: boolean;
  has_ignored_files: boolean;
  is_locked: boolean;
  lock_reason: string | null;
  /** HEAD as of the scan, sent back on removal so the backend can spot a newer commit. */
  head: string;
  state: WorktreeState;
  is_detached: boolean;
  /** Untracked OS/watcher noise (.DS_Store, watcher cookies) — blocks Git, but is not work. */
  junk_file_count: number;
  stale_reason: string | null;
}

export interface WorktreeScanResult {
  worktrees: MergedWorktree[];
  total_size: number;
  scan_path: string;
  warnings: string[];
  /** Non-failure notes, e.g. which base branches each repository was compared against. */
  diagnostics: string[];
}

export interface WorktreeRemoval {
  repository_path: string;
  worktree_path: string;
  head: string;
  /** Consent to lose uncommitted changes. Only ever true for rows that were dirty at scan time. */
  force: boolean;
}

export interface WorktreeDeleteResult {
  success: boolean;
  path: string;
  error: string | null;
  /** Something else removed it after the scan. Counts as success. */
  already_gone: boolean;
}

/** Persisted across runs, unlike everything else in this app. */
export interface Settings {
  /** macOS only; stored but inert elsewhere. */
  hide_dock: boolean;
  /** Folders the menu bar counts worktrees and free space for. */
  watched_folders: string[];
}

/** What the menu bar showed after its latest refresh, per watched folder. */
export interface TrayStats {
  folders: Array<{ folder: string; removable_worktrees: number }>;
  removable_worktrees: number;
}

/** What one menu bar refresh cost. Newest last. */
export interface TickDiagnostics {
  duration_ms: number;
  git_spawns: number;
  cache_hits: number;
  cache_considered: number;
  removable_worktrees: number;
}

export type SortField = 'name' | 'size' | 'manager';
export type SortDirection = 'asc' | 'desc';

export interface SortConfig {
  field: SortField;
  direction: SortDirection;
}
