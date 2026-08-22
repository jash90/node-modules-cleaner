import { useCallback, useMemo, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { CacheCleanResult, CacheScanResult, CacheTarget } from '../types';

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

  const cleanSelected = useCallback(async () => {
    if (isCleaning || isScanning || selectedIds.size === 0) return;

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

      // A prune leaves the directory in place with less inside, so re-measure rather than
      // dropping rows: only entries that vanished entirely should disappear from the list.
      const cleanedPaths = new Set(
        results.filter((result) => result.success).map((result) => result.path),
      );
      setTargets((current) => current.filter((target) => !cleanedPaths.has(target.path)));
      setSelectedIds(new Set());

      const failures = results.filter((result) => !result.success);
      if (failures.length > 0) {
        const first = failures[0].error ? ` ${failures[0].error}` : '';
        setError(`Failed to clean ${failures.length} target(s).${first}`);
      }
    } catch (err) {
      setError(`Cache cleanup failed: ${err}`);
    } finally {
      setIsCleaning(false);
    }
  }, [isCleaning, isScanning, keyOf, selectedIds, targets]);

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
    freedBytes,
    totalReclaimable,
    selectedReclaimable,
    selectionHasEstimate,
    scan,
    toggleSelection,
    selectSafe,
    deselectAll,
    cleanSelected,
    clearError: () => setError(null),
  };
}
