import { useState, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { NodeModulesFolder, ScanResult, DeleteResult, SortConfig } from '../types';

export function useNodeModules() {
  const [folders, setFolders] = useState<NodeModulesFolder[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [isScanning, setIsScanning] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [scanPath, setScanPath] = useState<string | null>(null);
  const [totalSize, setTotalSize] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [sortConfig, setSortConfig] = useState<SortConfig>({ field: 'size', direction: 'desc' });
  const [deleteResults, setDeleteResults] = useState<DeleteResult[]>([]);

  const sortedFolders = useMemo(() => {
    const sorted = [...folders].sort((a, b) => {
      let comparison = 0;

      switch (sortConfig.field) {
        case 'name':
          comparison = a.parent_project.localeCompare(b.parent_project);
          break;
        case 'size':
          comparison = a.size - b.size;
          break;
        case 'path':
          comparison = a.path.localeCompare(b.path);
          break;
      }

      return sortConfig.direction === 'asc' ? comparison : -comparison;
    });

    return sorted;
  }, [folders, sortConfig]);

  const selectDirectory = useCallback(async () => {
    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select folder to scan for node_modules',
      });

      if (selected) {
        return selected as string;
      }
      return null;
    } catch (err) {
      setError(`Failed to open directory picker: ${err}`);
      return null;
    }
  }, []);

  const scan = useCallback(async (path?: string) => {
    const targetPath = path || (await selectDirectory());

    if (!targetPath) return;

    setIsScanning(true);
    setError(null);
    setFolders([]);
    setSelectedPaths(new Set());
    setDeleteResults([]);
    setScanPath(targetPath);

    try {
      const result = await invoke<ScanResult>('scan_for_node_modules', { path: targetPath });
      setFolders(result.folders);
      setTotalSize(result.total_size);
    } catch (err) {
      setError(`Scan failed: ${err}`);
    } finally {
      setIsScanning(false);
    }
  }, [selectDirectory]);

  const toggleSelection = useCallback((path: string) => {
    setSelectedPaths(prev => {
      const newSet = new Set(prev);
      if (newSet.has(path)) {
        newSet.delete(path);
      } else {
        newSet.add(path);
      }
      return newSet;
    });
  }, []);

  const selectAll = useCallback(() => {
    setSelectedPaths(new Set(folders.map(f => f.path)));
  }, [folders]);

  const deselectAll = useCallback(() => {
    setSelectedPaths(new Set());
  }, []);

  const deleteSelected = useCallback(async () => {
    if (selectedPaths.size === 0) return;

    setIsDeleting(true);
    setError(null);

    try {
      const paths = Array.from(selectedPaths);
      const results = await invoke<DeleteResult[]>('delete_folders', { paths });
      setDeleteResults(results);

      // Remove successfully deleted folders from the list
      const successfullyDeleted = new Set(
        results.filter(r => r.success).map(r => r.path)
      );

      setFolders(prev => prev.filter(f => !successfullyDeleted.has(f.path)));
      setSelectedPaths(prev => {
        const newSet = new Set(prev);
        successfullyDeleted.forEach(path => newSet.delete(path));
        return newSet;
      });

      // Update total size
      const deletedSize = folders
        .filter(f => successfullyDeleted.has(f.path))
        .reduce((sum, f) => sum + f.size, 0);
      setTotalSize(prev => prev - deletedSize);

      // Check for errors
      const errors = results.filter(r => !r.success);
      if (errors.length > 0) {
        setError(`Failed to delete ${errors.length} folder(s). Check permissions.`);
      }
    } catch (err) {
      setError(`Delete operation failed: ${err}`);
    } finally {
      setIsDeleting(false);
    }
  }, [selectedPaths, folders]);

  const setSort = useCallback((field: SortConfig['field']) => {
    setSortConfig(prev => ({
      field,
      direction: prev.field === field && prev.direction === 'desc' ? 'asc' : 'desc',
    }));
  }, []);

  const selectedSize = useMemo(() => {
    return folders
      .filter(f => selectedPaths.has(f.path))
      .reduce((sum, f) => sum + f.size, 0);
  }, [folders, selectedPaths]);

  return {
    folders: sortedFolders,
    selectedPaths,
    isScanning,
    isDeleting,
    scanPath,
    totalSize,
    selectedSize,
    error,
    sortConfig,
    deleteResults,
    scan,
    toggleSelection,
    selectAll,
    deselectAll,
    deleteSelected,
    setSort,
    clearError: () => setError(null),
  };
}
