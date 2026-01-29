import { useState } from 'react';
import { useNodeModules } from './hooks/useNodeModules';
import { FolderList } from './components/FolderList';
import { SortControls } from './components/SortControls';
import { SizeDisplay, formatSize } from './components/SizeDisplay';
import { ConfirmDialog } from './components/ConfirmDialog';

function App() {
  const {
    folders,
    selectedPaths,
    isScanning,
    isDeleting,
    scanPath,
    totalSize,
    selectedSize,
    error,
    sortConfig,
    scan,
    toggleSelection,
    selectAll,
    deselectAll,
    deleteSelected,
    setSort,
    clearError,
  } = useNodeModules();

  const [showConfirmDialog, setShowConfirmDialog] = useState(false);

  const handleDelete = () => {
    setShowConfirmDialog(true);
  };

  const confirmDelete = () => {
    setShowConfirmDialog(false);
    deleteSelected();
  };

  return (
    <div className="h-screen flex flex-col bg-gray-50">
      {/* Header */}
      <header className="bg-white border-b border-gray-200 px-6 py-4">
        <div className="flex items-center justify-between">
          <div>
            <h1 className="text-xl font-bold text-gray-900">Node Modules Cleaner</h1>
            <p className="text-sm text-gray-500">
              Find and remove node_modules to free up disk space
            </p>
          </div>
          <button
            onClick={() => scan()}
            disabled={isScanning}
            className="px-4 py-2 bg-blue-600 text-white font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-2"
          >
            {isScanning ? (
              <>
                <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
                  <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                  <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                </svg>
                Scanning...
              </>
            ) : (
              <>
                <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                </svg>
                Select Folder
              </>
            )}
          </button>
        </div>
      </header>

      {/* Error Banner */}
      {error && (
        <div className="bg-red-50 border-b border-red-200 px-6 py-3 flex items-center justify-between">
          <span className="text-red-700 text-sm">{error}</span>
          <button
            onClick={clearError}
            className="text-red-500 hover:text-red-700"
          >
            <svg className="w-5 h-5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>
      )}

      {/* Stats Bar */}
      {scanPath && (
        <div className="bg-white border-b border-gray-200 px-6 py-3">
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-6 text-sm">
              <div>
                <span className="text-gray-500">Scanned: </span>
                <span className="font-medium text-gray-900 truncate max-w-md inline-block align-bottom" title={scanPath}>
                  {scanPath}
                </span>
              </div>
              <div>
                <span className="text-gray-500">Found: </span>
                <span className="font-medium text-gray-900">{folders.length} folders</span>
              </div>
              <div>
                <span className="text-gray-500">Total size: </span>
                <SizeDisplay bytes={totalSize} className="font-medium" />
              </div>
            </div>
            <SortControls sortConfig={sortConfig} onSort={setSort} />
          </div>
        </div>
      )}

      {/* Main Content */}
      <main className="flex-1 overflow-hidden p-6">
        {!scanPath ? (
          <div className="h-full flex flex-col items-center justify-center text-gray-500">
            <svg className="w-16 h-16 mb-4 text-gray-300" fill="none" viewBox="0 0 24 24" stroke="currentColor">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M3 7v10a2 2 0 002 2h14a2 2 0 002-2V9a2 2 0 00-2-2h-6l-2-2H5a2 2 0 00-2 2z" />
            </svg>
            <p className="text-lg font-medium mb-1">No folder selected</p>
            <p className="text-sm">Click "Select Folder" to scan for node_modules</p>
          </div>
        ) : isScanning ? (
          <div className="h-full flex flex-col items-center justify-center text-gray-500">
            <svg className="animate-spin h-12 w-12 mb-4 text-blue-600" viewBox="0 0 24 24">
              <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
              <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
            </svg>
            <p className="text-lg font-medium mb-1">Scanning...</p>
            <p className="text-sm">Looking for node_modules folders</p>
          </div>
        ) : (
          <div className="h-full bg-white rounded-lg border border-gray-200 shadow-sm overflow-hidden">
            <FolderList
              folders={folders}
              selectedPaths={selectedPaths}
              onToggleSelection={toggleSelection}
              onSelectAll={selectAll}
              onDeselectAll={deselectAll}
            />
          </div>
        )}
      </main>

      {/* Footer Actions */}
      {selectedPaths.size > 0 && (
        <footer className="bg-white border-t border-gray-200 px-6 py-4">
          <div className="flex items-center justify-between">
            <div className="text-sm">
              <span className="text-gray-500">Selected: </span>
              <span className="font-medium text-gray-900">{selectedPaths.size} folders</span>
              <span className="text-gray-300 mx-2">|</span>
              <span className="text-gray-500">Space to free: </span>
              <span className="font-medium text-green-600">{formatSize(selectedSize)}</span>
            </div>
            <button
              onClick={handleDelete}
              disabled={isDeleting}
              className="px-4 py-2 bg-red-600 text-white font-medium rounded-lg hover:bg-red-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center gap-2"
            >
              {isDeleting ? (
                <>
                  <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                  </svg>
                  Deleting...
                </>
              ) : (
                <>
                  <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 7l-.867 12.142A2 2 0 0116.138 21H7.862a2 2 0 01-1.995-1.858L5 7m5 4v6m4-6v6m1-10V4a1 1 0 00-1-1h-4a1 1 0 00-1 1v3M4 7h16" />
                  </svg>
                  Delete Selected
                </>
              )}
            </button>
          </div>
        </footer>
      )}

      {/* Confirm Dialog */}
      <ConfirmDialog
        isOpen={showConfirmDialog}
        selectedCount={selectedPaths.size}
        selectedSize={selectedSize}
        onConfirm={confirmDelete}
        onCancel={() => setShowConfirmDialog(false)}
      />
    </div>
  );
}

export default App;
