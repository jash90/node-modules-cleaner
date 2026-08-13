import { useCallback, useMemo, useRef, useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { useMergedWorktrees } from './useMergedWorktrees';
import { useNodeModules } from './useNodeModules';
import {
  createCleanupSummary,
  runCleanupDeletion,
  runCleanupScans,
} from '../utils/cleanupSummary';
import type { NodeModulesFolder } from '../types';

export function useCleanup() {
  const nodeModules = useNodeModules();
  const mergedWorktrees = useMergedWorktrees();
  const scanNodeModules = nodeModules.scan;
  const scanMergedWorktrees = mergedWorktrees.scan;
  const deleteNodeModules = nodeModules.deleteSelected;
  const reconcileNodeModules = nodeModules.reconcileAfterCleanup;
  const deleteMergedWorktrees = mergedWorktrees.deleteSelected;
  const clearNodeModulesError = nodeModules.clearError;
  const clearMergedWorktreesError = mergedWorktrees.clearError;
  const [scanPath, setScanPath] = useState<string | null>(null);
  const [isSelectingDirectory, setIsSelectingDirectory] = useState(false);
  const [isCoordinatingDeletion, setIsCoordinatingDeletion] = useState(false);
  const [pickerError, setPickerError] = useState<string | null>(null);
  const deletionInProgress = useRef(false);

  const isScanning = isSelectingDirectory
    || nodeModules.isScanning
    || mergedWorktrees.isScanning;
  const isDeleting = isCoordinatingDeletion
    || nodeModules.isDeleting
    || mergedWorktrees.isDeleting;

  const summary = useMemo(() => createCleanupSummary({
    nodeModules: nodeModules.folders.filter((folder) => (
      nodeModules.selectedPaths.has(folder.path)
    )),
    worktrees: mergedWorktrees.worktrees.filter((worktree) => (
      mergedWorktrees.selectedPaths.has(worktree.path)
    )),
  }), [
    mergedWorktrees.selectedPaths,
    mergedWorktrees.worktrees,
    nodeModules.folders,
    nodeModules.selectedPaths,
  ]);

  const availableSummary = useMemo(() => createCleanupSummary({
    nodeModules: nodeModules.folders,
    worktrees: mergedWorktrees.worktrees.filter((worktree) => (
      !worktree.is_dirty && !worktree.is_locked
    )),
  }), [mergedWorktrees.worktrees, nodeModules.folders]);

  const scan = useCallback(async () => {
    if (isScanning || isDeleting) return;

    setIsSelectingDirectory(true);
    setPickerError(null);

    try {
      const selected = await open({
        directory: true,
        multiple: false,
        title: 'Select folder to scan for cleanup candidates',
      });

      if (!selected) return;

      const path = selected as string;
      setScanPath(path);
      await runCleanupScans(path, scanNodeModules, scanMergedWorktrees);
    } catch (err) {
      setPickerError(`Failed to open directory picker: ${err}`);
    } finally {
      setIsSelectingDirectory(false);
    }
  }, [isDeleting, isScanning, scanMergedWorktrees, scanNodeModules]);

  const deleteSelected = useCallback(async () => {
    if (
      summary.totalCount === 0
      || deletionInProgress.current
      || isDeleting
      || isScanning
    ) return;

    const nodeModulePaths = nodeModules.folders
      .filter((folder) => nodeModules.selectedPaths.has(folder.path))
      .map((folder) => folder.path);
    const selectedWorktrees = mergedWorktrees.worktrees.filter((worktree) => (
      mergedWorktrees.selectedPaths.has(worktree.path)
    ));

    deletionInProgress.current = true;
    setIsCoordinatingDeletion(true);

    let deletedFolders: NodeModulesFolder[] = [];

    try {
      await runCleanupDeletion(
        () => deleteNodeModules(nodeModulePaths),
        (folders) => {
          deletedFolders = folders;
        },
        () => deleteMergedWorktrees(selectedWorktrees, deletedFolders),
        (removedWorktreePaths) => {
          reconcileNodeModules(deletedFolders, removedWorktreePaths);
        },
      );
    } finally {
      deletionInProgress.current = false;
      setIsCoordinatingDeletion(false);
    }
  }, [
    isDeleting,
    isScanning,
    deleteMergedWorktrees,
    deleteNodeModules,
    mergedWorktrees.selectedPaths,
    mergedWorktrees.worktrees,
    nodeModules.folders,
    nodeModules.selectedPaths,
    reconcileNodeModules,
    summary.totalCount,
  ]);

  const clearError = useCallback(() => {
    setPickerError(null);
    clearNodeModulesError();
    clearMergedWorktreesError();
  }, [clearMergedWorktreesError, clearNodeModulesError]);

  const error = [pickerError, nodeModules.error, mergedWorktrees.error]
    .filter(Boolean)
    .join(' ') || null;

  return {
    nodeModules,
    mergedWorktrees,
    scanPath,
    isScanning,
    isDeleting,
    summary,
    totalSize: availableSummary.totalSize,
    error,
    scan,
    deleteSelected,
    clearError,
  };
}
