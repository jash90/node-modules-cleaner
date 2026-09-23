import { useCallback, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type {
  MergedWorktree,
  NodeModulesFolder,
  WorktreeDeleteResult,
  WorktreeRemoval,
  WorktreeScanResult,
} from '../types';
import {
  adjustWorktreeSizes,
  applyWorktreeFailures,
  describeFailures,
  isSelectableWorktree,
  worktreeFailures,
} from '../utils/cleanupSummary';

export function useMergedWorktrees() {
  const [worktrees, setWorktrees] = useState<MergedWorktree[]>([]);
  const [selectedPaths, setSelectedPaths] = useState<Set<string>>(new Set());
  const [isScanning, setIsScanning] = useState(false);
  const [isDeleting, setIsDeleting] = useState(false);
  const [scanPath, setScanPath] = useState<string | null>(null);
  const [totalSize, setTotalSize] = useState(0);
  const [error, setError] = useState<string | null>(null);
  // Distinct from `error`, which also carries per-repository warnings from a scan that worked.
  const [scanFailed, setScanFailed] = useState(false);
  const [diagnostics, setDiagnostics] = useState<string[]>([]);
  // Rows a removal refused because they stopped being safe to remove since the scan; the same
  // click would fail again, so they stay visible with the reason but cannot be selected.
  const [blockedReasons, setBlockedReasons] = useState<Map<string, string>>(new Map());

  const scan = useCallback(async (path: string) => {
    if (isScanning || isDeleting) return;

    setIsScanning(true);
    setError(null);
    setScanFailed(false);
    setWorktrees([]);
    setSelectedPaths(new Set());
    setTotalSize(0);
    setDiagnostics([]);
    setBlockedReasons(new Map());
    setScanPath(path);

    try {
      const result = await invoke<WorktreeScanResult>('scan_for_merged_worktrees', {
        path,
      });
      setWorktrees(result.worktrees);
      setTotalSize(result.total_size);
      setDiagnostics(result.diagnostics ?? []);
      if (result.warnings.length > 0) {
        setError(`${result.warnings.length} repository warning(s). ${result.warnings[0]}`);
      }
    } catch (err) {
      setError(`Worktree scan failed: ${err}`);
      setScanFailed(true);
    } finally {
      setIsScanning(false);
    }
  }, [isDeleting, isScanning]);

  const toggleSelection = useCallback((path: string) => {
    setSelectedPaths((current) => {
      const worktree = worktrees.find((item) => item.path === path);
      if (!worktree || !isSelectableWorktree(worktree) || blockedReasons.has(path)) return current;

      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, [blockedReasons, worktrees]);

  const selectAll = useCallback(() => {
    setSelectedPaths(new Set(
      worktrees
        .filter((worktree) => isSelectableWorktree(worktree) && !blockedReasons.has(worktree.path))
        .map((worktree) => worktree.path),
    ));
  }, [blockedReasons, worktrees]);

  const deselectAll = useCallback(() => {
    setSelectedPaths(new Set());
  }, []);

  const deleteSelected = useCallback(async (
    selectedWorktrees: MergedWorktree[],
    deletedFolders: NodeModulesFolder[],
  ): Promise<WorktreeDeleteResult[]> => {
    if (isDeleting || isScanning) return [];

    const adjustedWorktrees = adjustWorktreeSizes(worktrees, deletedFolders);
    const updateWorktreeState = (nextWorktrees: MergedWorktree[]) => {
      setWorktrees(nextWorktrees);
      setTotalSize(nextWorktrees
        .filter(isSelectableWorktree)
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
        // Sent so the backend can refuse a worktree someone has committed into since the scan.
        head: worktree.head,
        // Consent covers what the confirmation dialog listed: rows that were dirty when scanned.
        force: worktree.is_dirty,
      }));

    try {
      const results = await invoke<WorktreeDeleteResult[]>('delete_merged_worktrees', { removals });
      // Includes rows something else removed after the scan: they are gone either way, and
      // keeping them would make every later click fail on them again.
      const removedPaths = new Set(
        results.filter((result) => result.success).map((result) => result.path),
      );
      const { kept, blocked } = applyWorktreeFailures(
        adjustedWorktrees.filter((worktree) => !removedPaths.has(worktree.path)),
        results,
      );

      updateWorktreeState(kept);
      setBlockedReasons((current) => new Map([...current, ...blocked]));
      const keptPaths = new Set(kept.map((worktree) => worktree.path));
      setSelectedPaths((current) => new Set(
        Array.from(current).filter((path) => keptPaths.has(path) && !blocked.has(path)),
      ));

      setError(describeFailures('worktree', worktreeFailures(results)));
      return results;
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
    scanFailed,
    diagnostics,
    blockedReasons,
    scan,
    toggleSelection,
    selectAll,
    deselectAll,
    deleteSelected,
    clearError: () => setError(null),
  };
}
