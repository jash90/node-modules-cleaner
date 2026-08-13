import { useState, useCallback, useMemo } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { NodeModulesFolder, ScanResult, DeleteResult, SortConfig } from '../types';
import { removeCandidatesWithinPaths } from '../utils/cleanupSummary';

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
        case 'manager':
          comparison = a.package_manager.localeCompare(b.package_manager);
          break;
      }

      return sortConfig.direction === 'asc' ? comparison : -comparison;
    });

    return sorted;
  }, [folders, sortConfig]);

  const scan = useCallback(async (path: string) => {
    if (isScanning || isDeleting) return;

    setIsScanning(true);
    setError(null);
    setFolders([]);
    setSelectedPaths(new Set());
    setDeleteResults([]);
    setTotalSize(0);
    setScanPath(path);

    try {
      const result = await invoke<ScanResult>('scan_for_node_modules', { path });
      setFolders(result.folders);
      setTotalSize(result.total_size);
    } catch (err) {
      setError(`Scan failed: ${err}`);
    } finally {
      setIsScanning(false);
    }
  }, [isDeleting, isScanning]);

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

  const deleteSelected = useCallback(async (
    paths: string[],
  ): Promise<NodeModulesFolder[]> => {
    if (paths.length === 0 || isDeleting || isScanning) return [];

    setIsDeleting(true);
    setError(null);

    try {
      const selectedPathSet = new Set(paths);
      const deletionTargets = folders.filter((folder) => selectedPathSet.has(folder.path));
      const results = await invoke<DeleteResult[]>('delete_folders', { paths });
      setDeleteResults(results);

      // Remove successfully deleted folders from the list
      const successfullyDeleted = new Set<string>();
      results.forEach((result) => {
        if (result.success) successfullyDeleted.add(result.path);
      });
      const deletedFolders = deletionTargets.filter((folder) => (
        successfullyDeleted.has(folder.path)
      ));

      setFolders(prev => prev.filter(f => !successfullyDeleted.has(f.path)));
      setSelectedPaths(prev => {
        const newSet = new Set(prev);
        successfullyDeleted.forEach(path => newSet.delete(path));
        return newSet;
      });

      // Update total size
      const deletedSize = deletedFolders
        .reduce((sum, f) => sum + f.size, 0);
      setTotalSize(prev => Math.max(0, prev - deletedSize));

      // Check for errors
      const errors = results.filter(r => !r.success);
      if (errors.length > 0) {
        setError(`Failed to delete ${errors.length} folder(s). Check permissions.`);
      }
      return deletedFolders;
    } catch (err) {
      setError(`Delete operation failed: ${err}`);
      return [];
    } finally {
      setIsDeleting(false);
    }
  }, [folders, isDeleting, isScanning]);

  const setSort = useCallback((field: SortConfig['field']) => {
    setSortConfig(prev => ({
      field,
      direction: prev.field === field && prev.direction === 'desc' ? 'asc' : 'desc',
    }));
  }, []);

  const reconcileAfterCleanup = useCallback((
    deletedFolders: NodeModulesFolder[],
    removedWorktreePaths: string[],
  ) => {
    const deletedPaths = new Set(deletedFolders.map((folder) => folder.path));
    const afterFolderDeletion = folders.filter((folder) => !deletedPaths.has(folder.path));
    const remainingFolders = removeCandidatesWithinPaths(
      afterFolderDeletion,
      removedWorktreePaths,
    );
    const remainingPaths = new Set(remainingFolders.map((folder) => folder.path));

    setFolders(remainingFolders);
    setSelectedPaths((current) => new Set(
      Array.from(current).filter((path) => remainingPaths.has(path)),
    ));
    setTotalSize(remainingFolders.reduce((total, folder) => total + folder.size, 0));
  }, [folders]);

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
    reconcileAfterCleanup,
    setSort,
    clearError: () => setError(null),
  };
}
