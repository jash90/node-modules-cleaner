import type { CacheKind, CacheTarget } from '../types';
import { formatSize } from '../utils/formatSize';
import { SizeDisplay } from './SizeDisplay';

interface CacheListProps {
  targets: CacheTarget[];
  selectedPaths: Set<string>;
  warnings: string[];
  isScanning: boolean;
  onToggleSelection: (path: string) => void;
  onSelectSafe: () => void;
  onDeselectAll: () => void;
  onScan: () => void;
  selectionDisabled?: boolean;
}

const GROUPS: { kind: CacheKind; title: string; blurb: string }[] = [
  {
    kind: 'orphaned_store',
    title: 'Abandoned stores',
    blurb: 'Left behind when a tool moved its default location. Nothing reads these.',
  },
  {
    kind: 'package_manager',
    title: 'Package manager caches',
    blurb: 'Re-downloaded on demand. Costs bandwidth, never breaks a project.',
  },
  {
    kind: 'runtime',
    title: 'Superseded runtimes',
    blurb: 'One copy per version. The newest of each is kept.',
  },
  {
    kind: 'log',
    title: 'Unrotated logs',
    blurb: 'Emptied in place, so the running service keeps its file handle.',
  },
];

function DatabaseIcon({ className }: { className?: string }) {
  return (
    <svg className={className} viewBox="0 0 24 24" fill="none" stroke="currentColor" aria-hidden="true">
      <ellipse cx="12" cy="6" rx="7" ry="3" strokeWidth="1.8" />
      <path d="M5 6v6c0 1.66 3.13 3 7 3s7-1.34 7-3V6" strokeWidth="1.8" />
      <path d="M5 12v6c0 1.66 3.13 3 7 3s7-1.34 7-3v-6" strokeWidth="1.8" />
    </svg>
  );
}

function relativeAge(unixSeconds: number | null): string | null {
  if (unixSeconds === null) return null;

  const days = Math.floor((Date.now() / 1000 - unixSeconds) / 86_400);
  if (days < 1) return 'touched today';
  if (days === 1) return 'untouched for a day';
  if (days < 30) return `untouched for ${days} days`;
  const months = Math.floor(days / 30);
  return months === 1 ? 'untouched for a month' : `untouched for ${months} months`;
}

/** How the target will be cleaned, in words the user can check against their own shell. */
function methodLabel(target: CacheTarget): string {
  switch (target.cleanup.type) {
    case 'external_command':
      return target.cleanup.display;
    case 'truncate_file':
      return 'empty in place';
    default:
      return 'delete';
  }
}

export function CacheList({
  targets,
  selectedPaths,
  warnings,
  isScanning,
  onToggleSelection,
  onSelectSafe,
  onDeselectAll,
  onScan,
  selectionDisabled = false,
}: CacheListProps) {
  const anySelected = targets.some((target) => selectedPaths.has(target.path));

  return (
    <div className="flex flex-col">
      <div className="flex items-center justify-between py-3 px-4 bg-sky-50/70 border-b border-sky-100 rounded-t-lg">
        <div className="flex items-center gap-2">
          <DatabaseIcon className="w-4 h-4 text-sky-700" />
          <span className="font-medium text-gray-800">Developer caches</span>
          <span className="text-xs text-gray-500">({targets.length})</span>
        </div>
        <div className="flex items-center gap-3">
          <button
            type="button"
            onClick={onScan}
            disabled={isScanning || selectionDisabled}
            className="text-xs font-medium text-sky-700 hover:text-sky-900 disabled:opacity-40 disabled:cursor-not-allowed"
          >
            {isScanning ? 'Scanning…' : 'Scan caches'}
          </button>
          {targets.length > 0 && (
            <button
              type="button"
              onClick={onSelectSafe}
              disabled={selectionDisabled}
              className="text-xs font-medium text-sky-700 hover:text-sky-900 disabled:opacity-40 disabled:cursor-not-allowed"
            >
              Select safe
            </button>
          )}
          {anySelected && (
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

      {warnings.length > 0 && (
        <div className="px-4 py-2 bg-amber-50 border-b border-amber-100 text-xs text-amber-800">
          {warnings.map((warning) => <p key={warning}>{warning}</p>)}
        </div>
      )}

      {targets.length === 0 ? (
        <div className="min-h-44 flex flex-col items-center justify-center text-gray-500">
          <DatabaseIcon className="w-10 h-10 mb-3 text-gray-300" />
          <p className="font-medium text-gray-700">
            {isScanning ? 'Looking for caches…' : 'No developer caches scanned yet'}
          </p>
          <p className="text-sm mt-1">
            These live outside your projects — npm, pnpm, bun, Gradle, uv, Node versions.
          </p>
        </div>
      ) : (
        GROUPS.map((group) => {
          const groupTargets = targets.filter((target) => target.kind === group.kind);
          if (groupTargets.length === 0) return null;

          // Sum whatever each row displays, so the header matches the numbers beneath it.
          const groupTotal = groupTargets.reduce(
            (sum, t) => sum + (t.cleanup.type === 'external_command' ? t.allocated_size : t.reclaimable_size),
            0,
          );

          return (
            <section key={group.kind}>
              <header className="px-4 py-2 bg-gray-50 border-b border-gray-100 flex items-baseline justify-between">
                <div>
                  <h3 className="text-sm font-semibold text-gray-800">{group.title}</h3>
                  <p className="text-[11px] text-gray-500">{group.blurb}</p>
                </div>
                <span className="text-xs text-gray-500">{formatSize(groupTotal)}</span>
              </header>

              <ul className="divide-y divide-gray-100">
                {groupTargets.map((target) => {
                  const isSelected = selectedPaths.has(target.path);
                  const needsReview = target.safety === 'needs_review';
                  const age = relativeAge(target.last_modified);
                  // A gap between nominal and reclaimable is the interesting case:
                  // hardlinked stores, sparse images, evicted iCloud files.
                  const understated = target.logical_size > target.reclaimable_size * 1.2;
                  // Handed to the tool's own prune: the row shows current size, not a saving.
                  const delegated = target.cleanup.type === 'external_command';

                  return (
                    <li key={target.path}>
                      <label
                        className={`flex items-center gap-4 px-4 py-3.5 transition-colors cursor-pointer ${
                          isSelected ? 'bg-sky-50' : 'hover:bg-gray-50'
                        }`}
                      >
                        <input
                          type="checkbox"
                          aria-label={`Select ${target.label}`}
                          checked={isSelected}
                          disabled={selectionDisabled}
                          onChange={() => onToggleSelection(target.path)}
                          className="w-4 h-4 rounded border-gray-300 text-sky-600 focus:ring-sky-500 disabled:opacity-40"
                        />

                        <div className="flex-1 min-w-0">
                          <div className="flex items-center gap-2 min-w-0">
                            <span className="font-semibold text-gray-900 truncate">{target.label}</span>
                            {needsReview && (
                              <span className="inline-flex items-center px-2 py-0.5 rounded-full bg-amber-100 text-amber-800 text-[10px] font-medium">
                                read the note
                              </span>
                            )}
                            <span className="inline-flex items-center px-2 py-0.5 rounded-full bg-gray-100 text-gray-600 text-[10px] font-mono">
                              {methodLabel(target)}
                            </span>
                            {age && <span className="text-[11px] text-gray-400">{age}</span>}
                          </div>

                          <p className="text-xs text-gray-400 truncate mt-1" title={target.path}>
                            {target.path}
                          </p>

                          {target.note && (
                            <p className="text-xs text-gray-500 mt-1">{target.note}</p>
                          )}
                        </div>

                        <div className="text-right whitespace-nowrap">
                          {delegated ? (
                            <>
                              <SizeDisplay bytes={target.allocated_size} className="block" />
                              <span
                                className="text-[10px] text-gray-400"
                                title="The tool keeps whatever is still referenced, so only part of this comes back"
                              >
                                prune frees part of this
                              </span>
                            </>
                          ) : (
                            <>
                              <SizeDisplay bytes={target.reclaimable_size} className="block" />
                              {understated && (
                                <span
                                  className="text-[10px] text-gray-400"
                                  title="Nominal size on disk — the difference is shared or sparse data that deleting will not release"
                                >
                                  of {formatSize(target.logical_size)} nominal
                                </span>
                              )}
                            </>
                          )}
                        </div>
                      </label>
                    </li>
                  );
                })}
              </ul>
            </section>
          );
        })
      )}
    </div>
  );
}
