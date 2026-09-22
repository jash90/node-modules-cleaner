export interface CleanupCandidate {
  path: string;
  size: number;
}

export interface CleanupSummaryInput {
  nodeModules: CleanupCandidate[];
  worktrees: CleanupCandidate[];
}

export interface CleanupSummaryItem {
  label: string;
  count: number;
}

export interface CleanupSummary {
  totalCount: number;
  totalSize: number;
  items: CleanupSummaryItem[];
}

export function createCleanupSummary(
  input: CleanupSummaryInput,
): CleanupSummary {
  const items = [
    { label: "node_modules folders", count: input.nodeModules.length },
    { label: "merged Git worktrees", count: input.worktrees.length },
  ].filter((item) => item.count > 0);

  const worktreeSize = input.worktrees.reduce(
    (total, worktree) => total + worktree.size,
    0,
  );
  const standaloneNodeModulesSize = input.nodeModules
    .filter((folder) => !input.worktrees.some(
      (worktree) => isNestedPath(folder.path, worktree.path),
    ))
    .reduce((total, folder) => total + folder.size, 0);

  return {
    totalCount: input.nodeModules.length + input.worktrees.length,
    totalSize: worktreeSize + standaloneNodeModulesSize,
    items,
  };
}

function normalizedPath(path: string): string {
  const normalized = path.replaceAll('\\', '/').replace(/\/+$/, '');
  return /^[A-Za-z]:\//.test(normalized) ? normalized.toLowerCase() : normalized;
}

function isNestedPath(path: string, parentPath: string): boolean {
  const child = normalizedPath(path);
  const parent = normalizedPath(parentPath);
  return child !== parent && child.startsWith(`${parent}/`);
}

export function adjustWorktreeSizes<T extends CleanupCandidate>(
  worktrees: T[],
  deletedFolders: CleanupCandidate[],
): T[] {
  return worktrees.map((worktree) => {
    const removedSize = deletedFolders
      .filter((folder) => isNestedPath(folder.path, worktree.path))
      .reduce((total, folder) => total + folder.size, 0);

    return removedSize === 0
      ? worktree
      : { ...worktree, size: Math.max(0, worktree.size - removedSize) };
  });
}

export function removeCandidatesWithinPaths<T extends CleanupCandidate>(
  candidates: T[],
  parentPaths: string[],
): T[] {
  return candidates.filter((candidate) => !parentPaths.some(
    (parentPath) => isNestedPath(candidate.path, parentPath),
  ));
}

export function runCleanupScans(
  path: string,
  scanNodeModules: (path: string) => Promise<void>,
  scanWorktrees: (path: string) => Promise<void>,
): Promise<PromiseSettledResult<void>[]> {
  return Promise.allSettled([
    scanNodeModules(path),
    scanWorktrees(path),
  ]);
}

export async function runCleanupDeletion<T, U>(
  deleteNodeModules: () => Promise<T>,
  adjustWorktrees: (deletedNodeModules: T) => void,
  deleteWorktrees: () => Promise<U>,
  reconcileNodeModules: (removedWorktrees: U) => void,
): Promise<void> {
  const deletedNodeModules = await deleteNodeModules();
  adjustWorktrees(deletedNodeModules);
  const removedWorktrees = await deleteWorktrees();
  reconcileNodeModules(removedWorktrees);
}

/** Only a lock rules a worktree out; uncommitted changes are offered behind a warning. */
export function isSelectableWorktree(worktree: { is_locked: boolean }): boolean {
  return !worktree.is_locked;
}

/**
 * Mirror of `counts_as_removable` in `git_worktrees.rs`, the rule behind the menu bar number.
 * Stale entries are selectable but free nothing, so the menu bar leaves them out.
 */
export function countRemovableWorktrees(
  worktrees: Array<{ is_locked: boolean; state: string }>,
): number {
  return worktrees.filter((worktree) => (
    isSelectableWorktree(worktree) && worktree.state !== 'stale'
  )).length;
}

export function worktreesNeedingConsent<T extends { is_dirty: boolean }>(worktrees: T[]): T[] {
  return worktrees.filter((worktree) => worktree.is_dirty);
}

/** The menu bar's count for `scanPath`, or `null` when that folder is not watched. */
export function trayCountFor(
  stats: { folders: Array<{ folder: string; removable_worktrees: number }> },
  scanPath: string,
): number | null {
  const target = normalizedPath(scanPath);
  const match = stats.folders.find((entry) => normalizedPath(entry.folder) === target);
  return match ? match.removable_worktrees : null;
}

export interface TrayMismatchInput {
  stats: { folders: Array<{ folder: string; removable_worktrees: number }> } | null;
  statsReceivedAt: number;
  listBuiltAt: number;
  scanPath: string | null;
  windowCount: number;
  isBusy: boolean;
  scanFailed: boolean;
}

/**
 * When the menu bar and the window disagree about the scanned folder, the window's list is the
 * one that aged. Only a count taken after the list was built says anything about it, and a
 * failed scan's empty list is an error the window already shows — not evidence of change.
 */
export function detectTrayMismatch(
  input: TrayMismatchInput,
): { trayCount: number; windowCount: number } | null {
  if (!input.stats || !input.scanPath || input.isBusy || input.scanFailed) return null;
  if (input.statsReceivedAt <= input.listBuiltAt) return null;
  const trayCount = trayCountFor(input.stats, input.scanPath);
  if (trayCount === null || trayCount === input.windowCount) return null;
  return { trayCount, windowCount: input.windowCount };
}
