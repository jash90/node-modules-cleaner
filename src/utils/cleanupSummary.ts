import type {
  CacheCleanResult,
  DeleteResult,
  WorktreeDeleteResult,
} from '../types';

export interface CleanupCandidate {
  path: string;
  size: number;
}

export interface CacheCandidate extends CleanupCandidate {
  /** Handed to a tool's own prune: what it frees is known only once it has run. */
  isEstimate?: boolean;
}

export interface CleanupSummaryInput {
  nodeModules: CleanupCandidate[];
  worktrees: CleanupCandidate[];
  caches?: CacheCandidate[];
}

export interface CleanupSummaryItem {
  label: string;
  count: number;
}

export interface CleanupSummary {
  totalCount: number;
  totalSize: number;
  items: CleanupSummaryItem[];
  /** The total is a floor: a selected cache is pruned by its own tool. */
  hasEstimate: boolean;
}

export function createCleanupSummary(
  input: CleanupSummaryInput,
): CleanupSummary {
  const caches = input.caches ?? [];
  const items = [
    { label: "node_modules folders", count: input.nodeModules.length },
    { label: "merged Git worktrees", count: input.worktrees.length },
    { label: "developer caches", count: caches.length },
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

  const cacheSize = caches.reduce((total, cache) => total + cache.size, 0);

  return {
    totalCount: input.nodeModules.length + input.worktrees.length + caches.length,
    totalSize: worktreeSize + standaloneNodeModulesSize + cacheSize,
    items,
    hasEstimate: caches.some((cache) => cache.isEstimate === true),
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
  worktrees: Array<{ path?: string; is_locked: boolean; state: string }>,
  // Rows whose removal was refused stay listed but are no longer removable — nor counted by the
  // menu bar, which rescans them.
  blocked: ReadonlyMap<string, string> = new Map(),
): number {
  return worktrees.filter((worktree) => (
    isSelectableWorktree(worktree)
    && worktree.state !== 'stale'
    && !(worktree.path !== undefined && blocked.has(worktree.path))
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

export interface FailureDetail {
  path: string;
  reason: string;
  sudoHint?: string | null;
}

/**
 * One line per failing path, so every failure says where and why — never only the first one.
 * Sudo hints repeat across results that share an owner, so each is shown once.
 */
export function describeFailures(noun: string, failures: FailureDetail[]): string | null {
  if (failures.length === 0) return null;

  const hints = Array.from(new Set(
    failures.map((failure) => failure.sudoHint).filter((hint): hint is string => Boolean(hint)),
  ));

  return [
    `Failed to remove ${failures.length} ${noun}(s):`,
    ...failures.map((failure) => `${failure.path}: ${failure.reason}`),
    ...hints.map((hint) => `Try: ${hint}`),
  ].join('\n');
}

/**
 * A partly deleted node_modules can leave thousands of files behind; the first one and a count
 * say enough to act on without flooding the banner.
 */
export function nodeModulesFailures(results: DeleteResult[]): FailureDetail[] {
  return results
    .filter((result) => !result.success)
    .map((result) => {
      const [first, ...rest] = result.failed_paths ?? [];
      const reason = first
        ? `${first.path}: ${first.reason}${rest.length > 0 ? ` (+${rest.length} more)` : ''}`
        : (result.error ?? 'Unknown error');
      return { path: result.path, reason, sudoHint: result.sudo_hint };
    });
}

export function worktreeFailures(results: WorktreeDeleteResult[]): FailureDetail[] {
  return results
    .filter((result) => !result.success)
    .map((result) => ({ path: result.path, reason: result.error ?? 'Unknown error' }));
}

export function cacheFailures(results: CacheCleanResult[]): FailureDetail[] {
  return results
    .filter((result) => !result.success)
    .map((result) => ({ path: result.path, reason: result.error ?? 'Unknown error' }));
}

export type WorktreeFailureKind = 'gone' | 'blocked' | 'retry';

/**
 * What a failed removal says about the row (messages from `remove_prepared_worktree` in
 * `git_worktrees.rs`). "No longer registered": the directory is no longer a worktree, so the
 * row describes nothing. "No longer merged" / "Changed since scan": still a worktree, but not a
 * safe one to remove — the same click would fail again. Anything else may pass on a retry.
 */
export function classifyWorktreeFailure(error: string | null): WorktreeFailureKind {
  if (!error) return 'retry';
  if (error.startsWith('No longer registered')) return 'gone';
  if (error.startsWith('No longer merged') || error.startsWith('Changed since scan')) {
    return 'blocked';
  }
  return 'retry';
}

export function applyWorktreeFailures<T extends { path: string }>(
  worktrees: T[],
  results: Array<{ path: string; success: boolean; error: string | null }>,
): { kept: T[]; blocked: Map<string, string> } {
  const gone = new Set<string>();
  const blocked = new Map<string, string>();

  for (const result of results) {
    if (result.success) continue;
    const kind = classifyWorktreeFailure(result.error);
    if (kind === 'gone') gone.add(result.path);
    if (kind === 'blocked' && result.error) blocked.set(result.path, result.error);
  }

  return { kept: worktrees.filter((worktree) => !gone.has(worktree.path)), blocked };
}

/**
 * Deleted and emptied targets leave the list; pruned ones stay, because the tool keeps what is
 * still referenced and the directory is still there.
 */
export function applyCacheCleanResults<T extends { path: string; cleanup: { type: string } }>(
  targets: T[],
  results: Array<{ path: string; success: boolean }>,
): { remaining: T[]; prunedPaths: string[] } {
  const succeeded = new Set(
    results.filter((result) => result.success).map((result) => result.path),
  );
  const pruned = targets.filter((target) => (
    succeeded.has(target.path) && target.cleanup.type === 'external_command'
  ));
  const prunedPaths = pruned.map((target) => target.path);

  return {
    remaining: targets.filter((target) => (
      !succeeded.has(target.path) || prunedPaths.includes(target.path)
    )),
    prunedPaths,
  };
}

export interface CategoryCounts {
  removed: number;
  failed: number;
  /** Bytes the backend reports as released, not the sizes estimated at scan time. */
  freedBytes: number;
}

export interface CategoryOutcome extends CategoryCounts {
  messages: string[];
}

export interface UnifiedCleanupReport {
  nodeModules: CategoryOutcome;
  worktrees: CategoryOutcome;
  caches: CategoryOutcome;
}

function crashed(requested: number, reason: unknown): CategoryOutcome {
  return { removed: 0, failed: requested, freedBytes: 0, messages: [String(reason)] };
}

/**
 * Runs the node_modules→worktrees chain and the cache cleanup side by side. Neither can stop
 * the other: a rejection is recorded against its own categories and the rest still report.
 */
export async function runUnifiedCleanup(
  runProjectCleanup: () => Promise<{ nodeModules: CategoryCounts; worktrees: CategoryCounts }>,
  runCacheCleanup: () => Promise<CategoryCounts>,
  requested: { nodeModules: number; worktrees: number; caches: number },
): Promise<UnifiedCleanupReport> {
  const [projects, caches] = await Promise.allSettled([
    runProjectCleanup(),
    runCacheCleanup(),
  ]);

  return {
    nodeModules: projects.status === 'fulfilled'
      ? { ...projects.value.nodeModules, messages: [] }
      : crashed(requested.nodeModules, projects.reason),
    worktrees: projects.status === 'fulfilled'
      ? { ...projects.value.worktrees, messages: [] }
      : crashed(requested.worktrees, projects.reason),
    caches: caches.status === 'fulfilled'
      ? { ...caches.value, messages: [] }
      : crashed(requested.caches, caches.reason),
  };
}

/** One line per category that was attempted, e.g. "node_modules: removed 3 · failed 1 · freed 1.2 GB". */
export function formatCleanupReport(
  report: UnifiedCleanupReport,
  formatBytes: (bytes: number) => string,
): string[] {
  const categories: Array<[string, CategoryOutcome]> = [
    ['node_modules', report.nodeModules],
    ['worktrees', report.worktrees],
    ['developer caches', report.caches],
  ];

  return categories
    .filter(([, outcome]) => outcome.removed + outcome.failed > 0 || outcome.messages.length > 0)
    .map(([label, outcome]) => {
      const parts = [`removed ${outcome.removed}`];
      if (outcome.failed > 0) parts.push(`failed ${outcome.failed}`);
      if (outcome.freedBytes > 0) parts.push(`freed ${formatBytes(outcome.freedBytes)}`);
      const why = outcome.messages.length > 0 ? ` — ${outcome.messages.join('; ')}` : '';
      return `${label}: ${parts.join(' · ')}${why}`;
    });
}

/**
 * Anything requested but not confirmed removed is a failure — including items a call that
 * threw never reported on. A partial delete still frees what it managed to remove.
 */
export function countOutcome(
  requested: number,
  results: Array<{ success: boolean; removed_bytes?: number }>,
): CategoryCounts {
  const removed = results.filter((result) => result.success).length;
  return {
    removed,
    failed: Math.max(0, requested - removed),
    freedBytes: results.reduce((total, result) => total + (result.removed_bytes ?? 0), 0),
  };
}
