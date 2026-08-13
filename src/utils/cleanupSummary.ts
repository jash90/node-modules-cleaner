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
