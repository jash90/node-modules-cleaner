export interface NodeModulesFolder {
  path: string;
  size: number;
  parent_project: string;
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

export type SortField = 'name' | 'size' | 'path';
export type SortDirection = 'asc' | 'desc';

export interface SortConfig {
  field: SortField;
  direction: SortDirection;
}
