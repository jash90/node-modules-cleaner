import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { useMergedWorktrees } from './useMergedWorktrees';
import { useNodeModules } from './useNodeModules';
import {
  countRemovableWorktrees,
  createCleanupSummary,
  detectTrayMismatch,
  isSelectableWorktree,
  runCleanupDeletion,
  runCleanupScans,
  worktreesNeedingConsent,
} from '../utils/cleanupSummary';
import type { NodeModulesFolder, TrayStats } from '../types';

/**
 * Ask the menu bar to recount. The count doubles as a check on the window's own list, and a
 * failed request is not worth surfacing: the next scheduled refresh arrives anyway.
 */
function refreshTray() {
  void invoke('refresh_tray_now').catch(() => {});
}

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
  // Menu bar counts from before the list was last rebuilt describe a different moment, so only
  // the ones that arrive afterwards are compared with it.
  const [trayStats, setTrayStats] = useState<{ stats: TrayStats; receivedAt: number } | null>(null);
  const [listBuiltAt, setListBuiltAt] = useState(0);

  useEffect(() => {
    const unlisten = listen<TrayStats>('tray-stats', (event) => {
      setTrayStats({ stats: event.payload, receivedAt: Date.now() });
    });
    return () => {
      void unlisten.then((stop) => stop());
    };
  }, []);

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
    worktrees: mergedWorktrees.worktrees.filter(isSelectableWorktree),
  }), [mergedWorktrees.worktrees, nodeModules.folders]);

  const selectedWorktreesNeedingConsent = useMemo(() => worktreesNeedingConsent(
    mergedWorktrees.worktrees.filter((worktree) => (
      mergedWorktrees.selectedPaths.has(worktree.path)
    )),
  ), [mergedWorktrees.selectedPaths, mergedWorktrees.worktrees]);

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
      setListBuiltAt(Date.now());
      refreshTray();
    } catch (err) {
      setPickerError(`Failed to open directory picker: ${err}`);
    } finally {
      setIsSelectingDirectory(false);
    }
  }, [isDeleting, isScanning, scanMergedWorktrees, scanNodeModules]);

  /** Scan the same folder again, without going through the picker. */
  const rescan = useCallback(async () => {
    if (!scanPath || isScanning || isDeleting) return;
    await runCleanupScans(scanPath, scanNodeModules, scanMergedWorktrees);
    setListBuiltAt(Date.now());
    refreshTray();
  }, [isDeleting, isScanning, scanMergedWorktrees, scanNodeModules, scanPath]);

  // The window list is a snapshot from the last scan; the menu bar recounts every fifteen
  // minutes. When both describe the same folder and disagree, the list is the one that aged.
  const windowCount = countRemovableWorktrees(mergedWorktrees.worktrees);
  const trayMismatch = detectTrayMismatch({
    stats: trayStats?.stats ?? null,
    statsReceivedAt: trayStats?.receivedAt ?? 0,
    listBuiltAt,
    scanPath,
    windowCount,
    isBusy: isScanning || isDeleting,
    scanFailed: mergedWorktrees.scanFailed,
  });

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
      setListBuiltAt(Date.now());
      // The menu bar counts are now stale by definition.
      refreshTray();
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
    selectedWorktreesNeedingConsent,
    trayMismatch,
    removableWorktreeCount: windowCount,
    error,
    scan,
    rescan,
    deleteSelected,
    clearError,
  };
}
