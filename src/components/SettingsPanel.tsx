import type { Settings } from '../types';

interface SettingsPanelProps {
  isOpen: boolean;
  settings: Settings;
  dockToggleAvailable: boolean;
  isBusy: boolean;
  error: string | null;
  onSetHideDock: (hidden: boolean) => void;
  onAddFolder: () => void;
  onRemoveFolder: (folder: string) => void;
  onClose: () => void;
}

function shortenHome(folder: string) {
  // Purely cosmetic; the full path stays in the title attribute.
  const match = /^\/Users\/[^/]+/.exec(folder) ?? /^\/home\/[^/]+/.exec(folder);
  return match ? `~${folder.slice(match[0].length)}` : folder;
}

export function SettingsPanel({
  isOpen,
  settings,
  dockToggleAvailable,
  isBusy,
  error,
  onSetHideDock,
  onAddFolder,
  onRemoveFolder,
  onClose,
}: SettingsPanelProps) {
  if (!isOpen) return null;

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center">
      <button
        type="button"
        aria-label="Close settings"
        className="absolute inset-0 bg-black/50"
        onClick={onClose}
      />

      <div className="relative bg-white rounded-xl shadow-2xl max-w-lg w-full mx-4 p-6">
        <h3 className="text-lg font-semibold text-gray-900">Settings</h3>
        <p className="text-sm text-gray-500 mt-1">
          The app keeps running in the menu bar after you close this window.
        </p>

        {error && (
          <p className="mt-4 text-sm text-red-700 bg-red-50 border border-red-200 rounded-lg px-3 py-2">
            {error}
          </p>
        )}

        <section className="mt-6">
          <label className="flex items-start gap-3">
            <input
              type="checkbox"
              checked={settings.hide_dock}
              disabled={!dockToggleAvailable || isBusy}
              onChange={(event) => onSetHideDock(event.target.checked)}
              className="mt-0.5 w-4 h-4 rounded border-gray-300 text-blue-600 focus:ring-blue-500 disabled:opacity-40"
            />
            <span className="min-w-0">
              <span className="block font-medium text-gray-900">Hide from the Dock</span>
              <span className="block text-xs text-gray-500 mt-0.5">
                {dockToggleAvailable
                  ? 'Runs as a menu bar app only. Takes effect immediately.'
                  : 'Only available on macOS — this platform has no Dock to hide from.'}
              </span>
            </span>
          </label>
        </section>

        <section className="mt-6">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <h4 className="font-medium text-gray-900">Watched folders</h4>
              <p className="text-xs text-gray-500 mt-0.5">
                The menu bar shows free disk space and removable worktrees for these.
              </p>
            </div>
            <button
              type="button"
              onClick={onAddFolder}
              disabled={isBusy}
              className="px-3 py-1.5 text-sm font-medium rounded-lg border border-gray-300 text-gray-700 hover:bg-gray-50 disabled:opacity-40 disabled:cursor-not-allowed whitespace-nowrap"
            >
              Add folder…
            </button>
          </div>

          {settings.watched_folders.length === 0 ? (
            <p className="mt-3 text-sm text-gray-500 bg-gray-50 rounded-lg px-3 py-3">
              Nothing watched yet — the menu bar shows just the icon.
            </p>
          ) : (
            <ul className="mt-3 divide-y divide-gray-100 border border-gray-200 rounded-lg">
              {settings.watched_folders.map((folder) => (
                <li key={folder} className="flex items-center gap-3 px-3 py-2">
                  <span
                    className="flex-1 min-w-0 truncate text-sm text-gray-700"
                    title={folder}
                  >
                    {shortenHome(folder)}
                  </span>
                  <button
                    type="button"
                    onClick={() => onRemoveFolder(folder)}
                    disabled={isBusy}
                    aria-label={`Stop watching ${folder}`}
                    className="text-xs font-medium text-gray-500 hover:text-red-600 disabled:opacity-40"
                  >
                    Remove
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <div className="mt-6 flex justify-end">
          <button
            type="button"
            onClick={onClose}
            className="px-4 py-2.5 rounded-lg bg-gray-900 text-white font-medium hover:bg-gray-800 transition-colors"
          >
            Done
          </button>
        </div>
      </div>
    </div>
  );
}
