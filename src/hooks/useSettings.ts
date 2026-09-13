import { useCallback, useEffect, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import type { Settings } from '../types';

const EMPTY_SETTINGS: Settings = { hide_dock: false, watched_folders: [] };

export function useSettings() {
  const [settings, setSettings] = useState<Settings>(EMPTY_SETTINGS);
  const [dockToggleAvailable, setDockToggleAvailable] = useState(false);
  const [isLoaded, setIsLoaded] = useState(false);
  const [isBusy, setIsBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // StrictMode runs effects twice in dev; the flag keeps the second pass from
    // overwriting state after unmount.
    let cancelled = false;

    void (async () => {
      try {
        const [loaded, dockAvailable] = await Promise.all([
          invoke<Settings>('get_settings'),
          invoke<boolean>('dock_toggle_available'),
        ]);
        if (cancelled) return;
        setSettings(loaded);
        setDockToggleAvailable(dockAvailable);
      } catch (err) {
        if (!cancelled) setError(`Could not read settings: ${err}`);
      } finally {
        if (!cancelled) setIsLoaded(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Every mutation returns the settings the backend actually persisted, so the UI
  // never drifts from the file on disk.
  const run = useCallback(async (
    action: () => Promise<Settings>,
    failure: string,
  ): Promise<void> => {
    setIsBusy(true);
    setError(null);
    try {
      setSettings(await action());
    } catch (err) {
      setError(`${failure}: ${err}`);
    } finally {
      setIsBusy(false);
    }
  }, []);

  const setHideDock = useCallback((hidden: boolean) => run(
    () => invoke<Settings>('set_hide_dock', { hidden }),
    'Could not change Dock visibility',
  ), [run]);

  const addFolder = useCallback(async () => {
    let selected: string | null = null;
    try {
      selected = (await open({
        directory: true,
        multiple: false,
        title: 'Select a folder for the menu bar to watch',
      })) as string | null;
    } catch (err) {
      setError(`Failed to open directory picker: ${err}`);
      return;
    }
    if (!selected) return;

    await run(
      () => invoke<Settings>('add_watched_folder', { folder: selected }),
      'Could not add the folder',
    );
  }, [run]);

  const removeFolder = useCallback((folder: string) => run(
    () => invoke<Settings>('remove_watched_folder', { folder }),
    'Could not remove the folder',
  ), [run]);

  return {
    settings,
    dockToggleAvailable,
    isLoaded,
    isBusy,
    error,
    setHideDock,
    addFolder,
    removeFolder,
    clearError: () => setError(null),
  };
}
