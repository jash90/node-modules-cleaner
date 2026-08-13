import type { NodeModulesFolder } from '../types';
import { SizeDisplay } from './SizeDisplay';

const BADGE_COLORS: Record<string, string> = {
  npm: 'bg-red-100 text-red-700',
  yarn: 'bg-blue-100 text-blue-700',
  pnpm: 'bg-amber-100 text-amber-700',
  bun: 'bg-yellow-100 text-yellow-700',
  unknown: 'bg-gray-100 text-gray-500',
};

function PackageManagerBadge({ manager }: { manager: string }) {
  const colors = BADGE_COLORS[manager] ?? BADGE_COLORS.unknown;
  return (
    <span className={`inline-flex items-center px-1.5 py-0.5 rounded text-[10px] font-medium leading-none ${colors}`}>
      {manager}
    </span>
  );
}

interface FolderListProps {
  folders: NodeModulesFolder[];
  selectedPaths: Set<string>;
  onToggleSelection: (path: string) => void;
  onSelectAll: () => void;
  onDeselectAll: () => void;
  selectionDisabled?: boolean;
}

export function FolderList({
  folders,
  selectedPaths,
  onToggleSelection,
  onSelectAll,
  onDeselectAll,
  selectionDisabled = false,
}: FolderListProps) {
  const allSelected = folders.length > 0 && folders.every(f => selectedPaths.has(f.path));
  const someSelected = folders.some(f => selectedPaths.has(f.path));

  return (
    <div className="flex flex-col">
      {/* Header */}
      <div className="flex items-center justify-between py-3 px-4 bg-gray-100 border-b border-gray-200 rounded-t-lg">
        <div className="flex items-center gap-3">
          <input
            type="checkbox"
            aria-label="Select all node_modules folders"
            disabled={selectionDisabled}
            checked={allSelected}
            ref={input => {
              if (input) {
                input.indeterminate = someSelected && !allSelected;
              }
            }}
            onChange={() => allSelected ? onDeselectAll() : onSelectAll()}
            className="w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 disabled:opacity-40"
          />
          <span className="text-sm font-medium text-gray-700">
            {selectedPaths.size} of {folders.length} selected
          </span>
        </div>
        <div className="flex gap-2">
          <button
            type="button"
            onClick={onSelectAll}
            disabled={selectionDisabled}
            className="text-xs text-blue-600 hover:text-blue-800 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Select All
          </button>
          <span className="text-gray-300">|</span>
          <button
            type="button"
            onClick={onDeselectAll}
            disabled={selectionDisabled}
            className="text-xs text-blue-600 hover:text-blue-800 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Deselect All
          </button>
        </div>
      </div>

      {/* List */}
      <div>
        {folders.length === 0 ? (
          <div className="min-h-40 flex items-center justify-center text-gray-500">
            No node_modules folders found
          </div>
        ) : (
          <ul className="divide-y divide-gray-100">
            {folders.map((folder) => (
              <li key={folder.path}>
                <label
                  className={`
                  flex items-center gap-4 px-4 py-3 transition-colors
                  ${selectionDisabled ? 'cursor-not-allowed opacity-70' : 'hover:bg-gray-50 cursor-pointer'}
                  ${selectedPaths.has(folder.path) ? 'bg-blue-50' : ''}
                `}
                >
                  <input
                    type="checkbox"
                    aria-label={`Select node_modules in ${folder.parent_project}`}
                    disabled={selectionDisabled}
                    checked={selectedPaths.has(folder.path)}
                    onChange={() => onToggleSelection(folder.path)}
                    className="w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 disabled:opacity-40"
                  />
                  <div className="flex-1 min-w-0">
                    <div className="flex items-center gap-2">
                      <span className="font-medium text-gray-900">
                        {folder.parent_project}
                      </span>
                      <PackageManagerBadge manager={folder.package_manager} />
                      <span className="text-gray-400">/</span>
                      <span className="text-gray-600 text-sm">node_modules</span>
                    </div>
                    <p className="text-xs text-gray-400 truncate mt-0.5" title={folder.path}>
                      {folder.path}
                    </p>
                    {folder.top_packages.length > 0 && (
                      <div className="flex gap-1 flex-wrap mt-1">
                        {folder.top_packages.map((pkg) => (
                          <span
                            key={pkg.name}
                            className="inline-flex items-center px-1.5 py-0.5 rounded bg-gray-100 text-gray-500 text-[10px] leading-none"
                          >
                            {pkg.name}
                          </span>
                        ))}
                      </div>
                    )}
                  </div>
                  <SizeDisplay bytes={folder.size} className="text-right whitespace-nowrap" />
                </label>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
