import { useCallback, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  DeleteResult,
  MergedWorktree,
  NodeModulesFolder,
  WorktreeRemoval,
  WorktreeScanResult,
} from '../types';
import { adjustWorktreeSizes } from '../utils/cleanupSummary';

export function useMergedWorktrees() {
  const [worktrees, setWorktrees] = useState<MergedWorktree[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [isScanning, setIsScanning] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [scanPath, setScanPath] = useState<string | null>(null);
  const [totalSize, setTotalSize] = useState(0);
  const [error, setError] = useState<string | null>(null);

  const scan = useCallback(async (path: string) => {
    if (isScanning || isDeleting) return;

    setIsScanning(true);
    setError(null);
    setWorktrees([]);
    setSelectedPaths(new Set());
    setTotalSize(0);
    setScanPath(path);

    try {
      const result = await invoke<WorktreeScanResult>('scan_for_merged_worktrees', {
        path,
      });
      setWorktrees(result.worktrees);
      setTotalSize(result.total_size);
      if (result.warnings.length > 0) {
        setError(`Skipped ${result.warnings.length} repository/repositories. ${result.warnings[0]}`);
      }
    } catch (err) {
      setError(`Worktree scan failed: ${err}`);
    } finally {
      setIsScanning(false);
    }
  }, [isDeleting, isScanning]);

  const toggleSelection = useCallback((path: string) => {
    setSelectedPaths((current) => {
      const worktree = worktrees.find((item) => item.path === path);
      if (!worktree || worktree.is_dirty || worktree.is_locked) return current;

      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, [worktrees]);

  const selectAll = useCallback(() => {
    setSelectedPaths(new Set(
      worktrees
        .filter((worktree) => !worktree.is_dirty && !worktree.is_locked)
        .map((worktree) => worktree.path),
    ));
  }, [worktrees]);

  const deselectAll = useCallback(() => {
    setSelectedPaths(new Set());
  }, []);

  const deleteSelected = useCallback(async (
    selectedWorktrees: MergedWorktree[],
    deletedFolders: NodeModulesFolder[],
  ): Promise<string[]> => {
    if (isDeleting || isScanning) return [];

    const adjustedWorktrees = adjustWorktreeSizes(worktrees, deletedFolders);
    const updateWorktreeState = (nextWorktrees: MergedWorktree[]) => {
      setWorktrees(nextWorktrees);
      setTotalSize(nextWorktrees
        .filter((worktree) => !worktree.is_dirty && !worktree.is_locked)
        .reduce((total, worktree) => total + worktree.size, 0));
    };

    if (selectedWorktrees.length === 0) {
      updateWorktreeState(adjustedWorktrees);
      return [];
    }

    setIsDeleting(true);
    setError(null);
    updateWorktreeState(adjustedWorktrees);

    const removals: WorktreeRemoval[] = selectedWorktrees
      .map((worktree) => ({
        repository_path: worktree.repository_path,
        worktree_path: worktree.path,
      }));

    try {
      const results = await invoke<DeleteResult[]>('delete_merged_worktrees', { removals });
      const removedPaths = new Set(
        results.filter((result) => result.success).map((result) => result.path),
      );
      const nextWorktrees = adjustedWorktrees.filter((worktree) => (
        !removedPaths.has(worktree.path)
      ));

      updateWorktreeState(nextWorktrees);
      setSelectedPaths((current) => {
        const next = new Set(current);
        removedPaths.forEach((path) => next.delete(path));
        return next;
      });

      const failures = results.filter((result) => !result.success);
      if (failures.length > 0) {
        const firstError = failures[0].error ? ` ${failures[0].error}` : '';
        setError(`Failed to remove ${failures.length} worktree(s).${firstError}`);
      }
      return Array.from(removedPaths);
    } catch (err) {
      setError(`Worktree removal failed: ${err}`);
      return [];
    } finally {
      setIsDeleting(false);
    }
  }, [isDeleting, isScanning, worktrees]);

  const selectedSize = useMemo(() => worktrees
    .filter((worktree) => selectedPaths.has(worktree.path))
    .reduce((sum, worktree) => sum + worktree.size, 0), [selectedPaths, worktrees]);

  return {
    worktrees,
    selectedPaths,
    isScanning,
    isDeleting,
    scanPath,
    totalSize,
    selectedSize,
    error,
    scan,
    toggleSelection,
    selectAll,
    deselectAll,
    deleteSelected,
    clearError: () => setError(null),
  };
}
