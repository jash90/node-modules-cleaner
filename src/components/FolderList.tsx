import type { NodeModulesFolder } from '../types';
import { SizeDisplay } from './SizeDisplay';

interface FolderListProps {
  folders: NodeModulesFolder[];
  selectedPaths: Set<string>;
  onToggleSelection: (path: string) => void;
  onSelectAll: () => void;
  onDeselectAll: () => void;
}

export function FolderList({
  folders,
  selectedPaths,
  onToggleSelection,
  onSelectAll,
  onDeselectAll,
}: FolderListProps) {
  const allSelected = folders.length > 0 && folders.every(f => selectedPaths.has(f.path));
  const someSelected = folders.some(f => selectedPaths.has(f.path));

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="flex items-center justify-between py-3 px-4 bg-gray-100 border-b border-gray-200 rounded-t-lg">
        <div className="flex items-center gap-3">
          <input
            type="checkbox"
            checked={allSelected}
            ref={input => {
              if (input) {
                input.indeterminate = someSelected && !allSelected;
              }
            }}
            onChange={() => allSelected ? onDeselectAll() : onSelectAll()}
            className="w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
          />
          <span className="text-sm font-medium text-gray-700">
            {selectedPaths.size} of {folders.length} selected
          </span>
        </div>
        <div className="flex gap-2">
          <button
            onClick={onSelectAll}
            className="text-xs text-blue-600 hover:text-blue-800"
          >
            Select All
          </button>
          <span className="text-gray-300">|</span>
          <button
            onClick={onDeselectAll}
            className="text-xs text-blue-600 hover:text-blue-800"
          >
            Deselect All
          </button>
        </div>
      </div>

      {/* List */}
      <div className="flex-1 overflow-y-auto">
        {folders.length === 0 ? (
          <div className="flex items-center justify-center h-full text-gray-500">
            No node_modules folders found
          </div>
        ) : (
          <ul className="divide-y divide-gray-100">
            {folders.map((folder) => (
              <li
                key={folder.path}
                className={`
                  flex items-center gap-4 px-4 py-3 hover:bg-gray-50 cursor-pointer transition-colors
                  ${selectedPaths.has(folder.path) ? 'bg-blue-50' : ''}
                `}
                onClick={() => onToggleSelection(folder.path)}
              >
                <input
                  type="checkbox"
                  checked={selectedPaths.has(folder.path)}
                  onChange={() => onToggleSelection(folder.path)}
                  onClick={(e) => e.stopPropagation()}
                  className="w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500"
                />
                <div className="flex-1 min-w-0">
                  <div className="flex items-center gap-2">
                    <span className="font-medium text-gray-900">
                      {folder.parent_project}
                    </span>
                    <span className="text-gray-400">/</span>
                    <span className="text-gray-600 text-sm">node_modules</span>
                  </div>
                  <p className="text-xs text-gray-400 truncate mt-0.5" title={folder.path}>
                    {folder.path}
                  </p>
                </div>
                <SizeDisplay bytes={folder.size} className="text-right whitespace-nowrap" />
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
