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

export interface MergedWorktree {
  path: string;
  branch: string;
  repository_path: string;
  repository_name: string;
  base_branch: string;
  size: number;
  is_dirty: boolean;
  has_ignored_files: boolean;
  is_locked: boolean;
  lock_reason: string | null;
}

export interface WorktreeScanResult {
  worktrees: MergedWorktree[];
  total_size: number;
  scan_path: string;
  warnings: string[];
}

export interface WorktreeRemoval {
  repository_path: string;
  worktree_path: string;
}

export type SortField = 'name' | 'size' | 'manager';
export type SortDirection = 'asc' | 'desc';

export interface SortConfig {
  field: SortField;
  direction: SortDirection;
}
