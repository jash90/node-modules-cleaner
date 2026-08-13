import type { MergedWorktree } from '../types';
import { SizeDisplay } from './SizeDisplay';

interface WorktreeListProps {
  worktrees: MergedWorktree[];
  selectedPaths: Set<string>;
  onToggleSelection: (path: string) => void;
  onSelectAll: () => void;
  onDeselectAll: () => void;
  selectionDisabled?: boolean;
}

function GitBranchIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" aria-hidden="true">
      <circle cx="6" cy="5" r="2.25" strokeWidth="1.8" />
      <circle cx="6" cy="19" r="2.25" strokeWidth="1.8" />
      <circle cx="18" cy="7" r="2.25" strokeWidth="1.8" />
      <path d="M6 7.5v9M8.25 17c5.6-.5 9.75-3.5 9.75-7.75" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

export function WorktreeList({
  worktrees,
  selectedPaths,
  onToggleSelection,
  onSelectAll,
  onDeselectAll,
  selectionDisabled = false,
}: WorktreeListProps) {
  const removable = worktrees.filter((worktree) => !worktree.is_dirty && !worktree.is_locked);
  const allSelected = removable.length > 0
    && removable.every((worktree) => selectedPaths.has(worktree.path));
  const someSelected = removable.some((worktree) => selectedPaths.has(worktree.path));

  return (
    <div className="flex flex-col">
      <div className="flex items-center justify-between py-3 px-4 bg-amber-50/70 border-b border-amber-100 rounded-t-lg">
        <div className="flex items-center gap-2">
          <GitBranchIcon className="w-4 h-4 text-amber-700" />
          <span className="font-medium text-gray-800">Merged Git worktrees</span>
          <span className="text-xs text-gray-500">({worktrees.length})</span>
        </div>
        <div className="flex items-center gap-3">
          {!allSelected && removable.length > 0 && (
            <button
              type="button"
              onClick={onSelectAll}
              disabled={selectionDisabled}
              className="text-xs font-medium text-amber-700 hover:text-amber-900 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Select removable
            </button>
          )}
          {someSelected && (
            <button
              type="button"
              onClick={onDeselectAll}
              disabled={selectionDisabled}
              className="text-xs font-medium text-gray-600 hover:text-gray-900 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Deselect all
            </button>
          )}
        </div>
      </div>

      <div>
        {worktrees.length === 0 ? (
          <div className="min-h-44 flex flex-col items-center justify-center text-gray-500">
            <GitBranchIcon className="w-10 h-10 mb-3 text-gray-300" />
            <p className="font-medium text-gray-700">No merged worktrees found</p>
            <p className="text-sm mt-1">Branches are compared with each repository&apos;s default branch.</p>
          </div>
        ) : (
          <ul className="divide-y divide-gray-100">
            {worktrees.map((worktree) => {
              const isSelected = selectedPaths.has(worktree.path);
              const isProtected = worktree.is_dirty || worktree.is_locked || selectionDisabled;
              return (
                <li key={`${worktree.repository_path}:${worktree.path}`}>
                  <label
                    className={`flex items-center gap-4 px-4 py-3.5 transition-colors ${
                    isProtected
                      ? 'bg-amber-50/40 cursor-not-allowed'
                      : isSelected
                        ? 'bg-amber-50 cursor-pointer'
                        : 'hover:bg-gray-50 cursor-pointer'
                  }`}
                  >
                    <input
                      type="checkbox"
                      aria-label={`Select ${worktree.branch}`}
                      checked={isSelected}
                      disabled={isProtected}
                      onChange={() => onToggleSelection(worktree.path)}
                      className="w-4 h-4 rounded border-gray-300 text-amber-600 focus:ring-amber-500 disabled:opacity-40"
                    />
                    <div className={`w-9 h-9 rounded-lg flex items-center justify-center ${
                      isProtected ? 'bg-amber-100 text-amber-700' : 'bg-gray-900 text-amber-300'
                    }`}>
                      <GitBranchIcon className="w-5 h-5" />
                    </div>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center gap-2 min-w-0">
                        <span className="font-semibold text-gray-900 truncate">{worktree.branch}</span>
                        <span
                          className="inline-flex items-center px-2 py-0.5 rounded-full bg-gray-100 text-gray-600 text-[10px] font-medium"
                          title={worktree.repository_path}
                        >
                          {worktree.repository_name}
                        </span>
                        <span className="text-[11px] text-gray-400">merged into {worktree.base_branch}</span>
                      </div>
                      <p className="text-xs text-gray-400 truncate mt-1" title={worktree.path}>
                        {worktree.path}
                      </p>
                      {worktree.is_dirty && (
                        <p className="text-xs font-medium text-amber-700 mt-1">
                          Uncommitted changes — removal disabled
                        </p>
                      )}
                      {worktree.is_locked && (
                        <p className="text-xs font-medium text-amber-700 mt-1">
                          Locked{worktree.lock_reason ? ` — ${worktree.lock_reason}` : ''} — removal disabled
                        </p>
                      )}
                      {!isProtected && worktree.has_ignored_files && (
                        <p className="text-xs font-medium text-amber-700 mt-1">
                          Contains ignored files — they will also be removed
                        </p>
                      )}
                    </div>
                    <SizeDisplay bytes={worktree.size} className="text-right whitespace-nowrap" />
                  </label>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
