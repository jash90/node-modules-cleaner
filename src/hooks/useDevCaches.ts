import { useCallback, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { CacheCleanResult, CacheScanResult, CacheTarget } from '../types';
import {
  applyCacheCleanResults,
  cacheFailures,
  describeFailures,
} from '../utils/cleanupSummary';

/**
 * Shared developer caches — package manager stores, versioned runtimes, unrotated logs.
 *
 * Unlike the node_modules and worktree scans this one takes no path: the targets live at
 * fixed, well-known locations rather than inside whatever folder the user picked.
 */
export function useDevCaches() {
  const [targets, setTargets] = useState<CacheTarget[]>([]);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [isScanning, setIsScanning] = useState(false);
  const [isCleaning, setIsCleaning] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [lastCleanup, setLastCleanup] = useState<CacheCleanResult[]>([]);
  // Pruned by their own tool: still on disk with less inside, so the row stays, marked.
  const [prunedPaths, setPrunedPaths] = useState<Set<string>>(new Set());

  // Paths are unique per target; ids are not (several Gradle versions share one id).
  const keyOf = useCallback((target: CacheTarget) => target.path, []);

  const scan = useCallback(async () => {
    if (isScanning || isCleaning) return;

    setIsScanning(true);
    setError(null);
    setTargets([]);
    setSelectedIds(new Set());
    setWarnings([]);
    setLastCleanup([]);
    setPrunedPaths(new Set());

    try {
      const result = await invoke<CacheScanResult>('scan_for_dev_caches');
      setTargets(result.targets);
      setWarnings(result.warnings);
    } catch (err) {
      setError(`Cache scan failed: ${err}`);
    } finally {
      setIsScanning(false);
    }
  }, [isCleaning, isScanning]);

  const toggleSelection = useCallback((path: string) => {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  /** Select everything the backend marked `safe`, leaving `needs_review` to a deliberate click. */
  const selectSafe = useCallback(() => {
    setSelectedIds(new Set(
      targets.filter((target) => target.safety === 'safe').map(keyOf),
    ));
  }, [keyOf, targets]);

  const deselectAll = useCallback(() => setSelectedIds(new Set()), []);

  /** The cache step of the unified cleanup; returns what the backend reported per target. */
  const cleanSelected = useCallback(async (): Promise<CacheCleanResult[]> => {
    if (isCleaning || isScanning || selectedIds.size === 0) return [];

    const selected = targets.filter((target) => selectedIds.has(keyOf(target)));

    setIsCleaning(true);
    setError(null);

    try {
      const results = await invoke<CacheCleanResult[]>('clean_dev_caches', {
        targets: selected.map((target) => ({
          id: target.id,
          path: target.path,
          cleanup: target.cleanup,
        })),
      });

      setLastCleanup(results);

      // Only entries that vanished or were emptied leave the list; a pruned directory is still
      // there, so its row stays, marked as pruned (its size is from before the prune).
      const { remaining, prunedPaths: pruned } = applyCacheCleanResults(targets, results);
      setTargets(remaining);
      setPrunedPaths((current) => new Set([...current, ...pruned]));
      setSelectedIds(new Set());

      setError(describeFailures('cache target', cacheFailures(results)));
      return results;
    } catch (err) {
      setError(`Cache cleanup failed: ${err}`);
      return [];
    } finally {
      setIsCleaning(false);
    }
  }, [isCleaning, isScanning, keyOf, selectedIds, targets]);

  const clearError = useCallback(() => setError(null), []);

  const totalReclaimable = useMemo(
    () => targets.reduce((sum, target) => sum + target.reclaimable_size, 0),
    [targets],
  );

  const selectedReclaimable = useMemo(
    () => targets
      .filter((target) => selectedIds.has(keyOf(target)))
      .reduce((sum, target) => sum + target.reclaimable_size, 0),
    [keyOf, selectedIds, targets],
  );

  /**
   * Whether any selected target is handed to a tool's own prune, whose saving cannot be
   * known before it runs — so the total shown alongside is a floor, not a promise.
   */
  const selectionHasEstimate = useMemo(
    () => targets.some((target) => (
      selectedIds.has(keyOf(target)) && target.cleanup.type === 'external_command'
    )),
    [keyOf, selectedIds, targets],
  );

  const freedBytes = useMemo(
    () => lastCleanup.reduce((sum, result) => sum + result.removed_bytes, 0),
    [lastCleanup],
  );

  return {
    targets,
    selectedIds,
    isScanning,
    isCleaning,
    warnings,
    error,
    lastCleanup,
    prunedPaths,
    freedBytes,
    totalReclaimable,
    selectedReclaimable,
    selectionHasEstimate,
    scan,
    toggleSelection,
    selectSafe,
    deselectAll,
    cleanSelected,
    clearError,
  };
}
