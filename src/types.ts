export interface TopPackage {
  name: string;
}

export interface NodeModulesFolder {
  path: string;
  size: number;
  parent_project: string;
  package_manager: string;
  top_packages: TopPackage[];
}

export interface ScanResult {
  folders: NodeModulesFolder[];
  total_size: number;
  scan_path: string;
}

export interface DeleteResult {
  success: boolean;
  path: string;
  error: string | null;
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
