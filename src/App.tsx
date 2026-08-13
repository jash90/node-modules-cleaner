import { useState } from 'react';
import { useCleanup } from './hooks/useCleanup';
import { FolderList } from './components/FolderList';
import { WorktreeList } from './components/WorktreeList';
import { SortControls } from './components/SortControls';
import { SizeDisplay } from './components/SizeDisplay';
import { ConfirmDialog } from './components/ConfirmDialog';
import { formatSize } from './utils/formatSize';

function Spinner({ className = 'h-4 w-4' }: { className?: string }) {
  return (
    <svg className={`animate-spin ${className}`} viewBox="0 0 24 24" aria-hidden="true">
      <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
      <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
    </svg>
  );
}

function FolderIcon({ className }: { className: string }) {
  return (
    <svg className={className} fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.7} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
    </svg>
  );
}

function BranchIcon({ className }: { className: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" aria-hidden="true">
      <circle cx="6" cy="5" r="2.25" strokeWidth="1.8" />
      <circle cx="6" cy="19" r="2.25" strokeWidth="1.8" />
      <circle cx="18" cy="7" r="2.25" strokeWidth="1.8" />
      <path d="M6 7.5v9M8.25 17c5.6-.5 9.75-3.5 9.75-7.75" strokeWidth="1.8" strokeLinecap="round" />
    </svg>
  );
}

function App() {
  const cleanup = useCleanup();
  const [showConfirmDialog, setShowConfirmDialog] = useState(false);
  const {
    nodeModules,
    mergedWorktrees,
    scanPath,
    isScanning,
    isDeleting,
    summary,
    totalSize,
    error,
  } = cleanup;

  const confirmDelete = () => {
    setShowConfirmDialog(false);
    void cleanup.deleteSelected();
  };

  return (
    <div className="h-screen flex flex-col bg-gray-50">
      <header className="bg-white border-b border-gray-200 px-6 py-4">
        <div className="flex items-center justify-between gap-6">
          <div className="min-w-0">
            <h1 className="text-xl font-bold text-gray-900">Node Modules Cleaner</h1>
            <p className="text-sm text-gray-500">
              Find and remove node_modules and merged Git worktrees
            </p>
          </div>
          <button
            type="button"
            onClick={() => void cleanup.scan()}
            disabled={isScanning || isDeleting}
            className="px-4 py-2 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-2"
          >
            {isScanning ? (
              <>
                <Spinner />
                Scanning...
              </>
            ) : (
              <>
                <FolderIcon className="w-4 h-4" />
                Select Folder
              </>
            )}
          </button>
        </div>
      </header>

      {error && (
        <div className="bg-red-50 border-b border-red-200 px-6 py-3 flex items-center justify-between gap-4">
          <span className="text-red-700 text-sm">{error}</span>
          <button
            type="button"
            onClick={cleanup.clearError}
            className="text-red-500 hover:text-red-700"
            aria-label="Dismiss error"
          >
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      )}

      {scanPath && (
        <div className="bg-white border-b border-gray-200 px-6 py-3">
          <div className="flex items-center gap-6 text-sm min-w-0">
            <div className="min-w-0">
              <span className="text-gray-500">Scanned: </span>
              <span className="font-medium text-gray-900 truncate max-w-md inline-block align-bottom" title={scanPath}>
                {scanPath}
              </span>
            </div>
            <div className="whitespace-nowrap">
              <span className="text-gray-500">Found: </span>
              <span className="font-medium text-gray-900">
                {nodeModules.folders.length} folders + {mergedWorktrees.worktrees.length} worktrees
              </span>
            </div>
            <div className="whitespace-nowrap">
              <span className="text-gray-500">Reclaimable: </span>
              <SizeDisplay bytes={totalSize} className="font-medium" />
            </div>
          </div>
        </div>
      )}

      <main className="flex-1 overflow-y-auto p-6">
        {!scanPath ? (
          <div className="min-h-full flex flex-col items-center justify-center text-gray-500">
            <div className="flex items-center gap-3 mb-4">
              <FolderIcon className="w-14 h-14 text-blue-200" />
              <BranchIcon className="w-14 h-14 text-amber-300" />
            </div>
            <p className="text-lg font-medium text-gray-700 mb-1">No folder selected</p>
            <p className="text-sm">Choose a folder to scan for node_modules and merged Git worktrees</p>
          </div>
        ) : isScanning ? (
          <div className="min-h-full flex flex-col items-center justify-center text-gray-500">
            <Spinner className="h-12 w-12 mb-4 text-blue-600" />
            <p className="text-lg font-medium text-gray-700 mb-1">Scanning...</p>
            <p className="text-sm">Looking for cleanup candidates</p>
          </div>
        ) : (
          <div className="space-y-6 pb-2">
            <section className="bg-white rounded-lg border border-blue-200 shadow-sm overflow-hidden">
              <div className="flex items-center justify-between gap-4 py-3 px-4 bg-blue-50/70 border-b border-blue-100">
                <div className="flex items-center gap-2">
                  <FolderIcon className="w-4 h-4 text-blue-700" />
                  <h2 className="font-medium text-gray-800">node_modules</h2>
                  <span className="text-xs text-gray-500">({nodeModules.folders.length})</span>
                </div>
                <SortControls sortConfig={nodeModules.sortConfig} onSort={nodeModules.setSort} />
              </div>
              <FolderList
                folders={nodeModules.folders}
                selectedPaths={nodeModules.selectedPaths}
                onToggleSelection={nodeModules.toggleSelection}
                onSelectAll={nodeModules.selectAll}
                onDeselectAll={nodeModules.deselectAll}
                selectionDisabled={isDeleting || isScanning}
              />
            </section>

            <section className="bg-white rounded-lg border border-amber-200 shadow-sm overflow-hidden">
              <WorktreeList
                worktrees={mergedWorktrees.worktrees}
                selectedPaths={mergedWorktrees.selectedPaths}
                onToggleSelection={mergedWorktrees.toggleSelection}
                onSelectAll={mergedWorktrees.selectAll}
                onDeselectAll={mergedWorktrees.deselectAll}
                selectionDisabled={isDeleting || isScanning}
              />
            </section>
          </div>
        )}
      </main>

      {summary.totalCount > 0 && (
        <footer className="bg-white border-t border-gray-200 px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="text-sm">
              <span className="text-gray-500">Selected: </span>
              <span className="font-medium text-gray-900">
                {summary.totalCount} items
              </span>
              <span className="text-gray-300 mx-2">|</span>
              <span className="text-gray-500">Space to free: </span>
              <span className="font-medium text-green-600">{formatSize(summary.totalSize)}</span>
            </div>
            <button
              type="button"
              onClick={() => setShowConfirmDialog(true)}
              disabled={isDeleting || isScanning}
              className="px-4 py-2 bg-red-600 text-white font-medium rounded-lg hover:bg-red-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-2"
            >
              {isDeleting ? (
                <>
                  <Spinner />
                  Removing...
                </>
              ) : (
                <>
                  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor" aria-hidden="true">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                  Remove Selected
                </>
              )}
            </button>
          </div>
        </footer>
      )}

      <ConfirmDialog
        isOpen={showConfirmDialog}
        title="Remove selected items?"
        description="node_modules folders are deleted permanently. Worktree files, including ignored files, are removed; Git branches are kept."
        items={summary.items}
        confirmLabel="Remove"
        selectedSize={summary.totalSize}
        onConfirm={confirmDelete}
        onCancel={() => setShowConfirmDialog(false)}
      />
    </div>
  );
}

export default App;
