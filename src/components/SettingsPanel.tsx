import { useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { Settings, TickDiagnostics } from '../types';

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

// The menu bar refresh runs unattended every fifteen minutes, so the only way to notice it
// getting slower is to record what it cost and show it somewhere. Mounted with the panel, so
// nothing is fetched while the dialog is closed.
function BackgroundRefresh() {
  const [ticks, setTicks] = useState<TickDiagnostics[] | null>(null);

  useEffect(() => {
    let cancelled = false;
    void invoke<TickDiagnostics[]>('tick_diagnostics')
      .then((result) => {
        if (!cancelled) setTicks(result);
      })
      .catch(() => {
        if (!cancelled) setTicks([]);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const latest = ticks?.[ticks.length - 1];

  return (
    <section className="mt-6">
      <h4 className="font-medium text-gray-900">Background refresh</h4>
      {!latest ? (
        <p className="mt-2 text-xs text-gray-500">
          {ticks === null ? 'Reading…' : 'No refresh has finished yet.'}
        </p>
      ) : (
        <>
          <p className="mt-2 text-sm text-gray-700">
            Last run took{' '}
            <span className="font-medium">{(latest.duration_ms / 1000).toFixed(1)} s</span> and
            started <span className="font-medium">{latest.git_spawns}</span> Git{' '}
            {latest.git_spawns === 1 ? 'process' : 'processes'}.
          </p>
          <p className="mt-0.5 text-xs text-gray-500">
            {latest.cache_considered === 0
              ? 'No worktrees to compare.'
              : `${latest.cache_hits} of ${latest.cache_considered} worktrees answered from the previous run.`}
          </p>
          {ticks.length > 1 && (
            <ul className="mt-2 flex flex-wrap gap-1.5">
              {ticks.slice(-8).map((tick, index) => (
                <li
                  key={`${index}-${tick.duration_ms}`}
                  className="text-[11px] tabular-nums text-gray-500 bg-gray-50 border border-gray-200 rounded px-1.5 py-0.5"
                  title={`${tick.git_spawns} Git processes, ${tick.removable_worktrees} removable`}
                >
                  {(tick.duration_ms / 1000).toFixed(1)}s
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
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

        <BackgroundRefresh />

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
