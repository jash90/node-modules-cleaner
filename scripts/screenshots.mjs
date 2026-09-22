// Regenerates the screenshots used by the GitHub Pages site in site/assets/screenshots/.
//
//   npx playwright install chromium   # once per machine
//   npm run screenshots
//
// The app is a Tauri desktop app, so its frontend normally talks to Rust through
// window.__TAURI_INTERNALS__. Here the real frontend runs in Vite's dev server inside
// headless Chromium, and that bridge is replaced with fixtures: every path, project and
// branch below is fictional, so no real machine data can leak into the images.
//
// Output is WebP when `cwebp` is on PATH (brew install webp), PNG otherwise.

import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, rmSync, statSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright';
import { createServer } from 'vite';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const outDir = join(root, 'site', 'assets', 'screenshots');
const tmpDir = join(root, 'node_modules', '.cache', 'screenshots');

const GB = 1024 ** 3;
const MB = 1024 ** 2;
const HOME = '/Users/dev';
const PROJECTS = `${HOME}/Projects`;
const DAY = 86_400;

// ---------------------------------------------------------------------------
// Fixtures — shapes mirror src/types.ts exactly (snake_case, as serde sends them).
// ---------------------------------------------------------------------------

function folder(project, manager, reclaimable, nominal, tech, subpath = '') {
  const parent = `${PROJECTS}/${project}${subpath}`;
  return {
    path: `${parent}/node_modules`,
    size: nominal,
    allocated_size: nominal,
    reclaimable_size: reclaimable,
    last_modified: null,
    parent_project: project,
    package_manager: manager,
    top_packages: tech.map((name) => ({ name })),
  };
}

const nodeModules = [
  folder('storefront-web', 'npm', 1.42 * GB, 1.42 * GB, ['next', 'react', 'typescript', 'tailwindcss']),
  folder('mobile-app', 'yarn', 912 * MB, 912 * MB, ['expo', 'react-native', 'typescript']),
  folder('admin-dashboard', 'pnpm', 64 * MB, 780 * MB, ['vite', 'react', 'typescript', 'tailwindcss']),
  folder('api-gateway', 'npm', 412 * MB, 412 * MB, ['@nestjs', 'typescript', 'esbuild']),
  folder('design-system', 'pnpm', 38 * MB, 655 * MB, ['react', 'vite', 'turbo']),
  folder('marketing-site', 'bun', 21 * MB, 318 * MB, ['astro', 'tailwindcss']),
  folder('legacy-crm', 'npm', 286 * MB, 286 * MB, ['@angular', 'webpack', 'typescript']),
  folder('edge-worker', 'bun', 58 * MB, 58 * MB, ['hono', 'typescript']),
  folder('docs-portal', 'yarn', 174 * MB, 174 * MB, ['gatsby', 'react']),
];

function worktree(repo, dir, branch, base, extra = {}) {
  return {
    path: `${PROJECTS}/${repo}.worktrees/${dir}`,
    branch,
    repository_path: `${PROJECTS}/${repo}`,
    repository_name: repo,
    base_branch: base,
    size: 0,
    is_dirty: false,
    has_ignored_files: false,
    is_locked: false,
    lock_reason: null,
    head: 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0',
    state: 'merged',
    is_detached: false,
    junk_file_count: 0,
    stale_reason: null,
    ...extra,
  };
}

const worktrees = [
  worktree('storefront-web', 'checkout-v2', 'feature/checkout-v2', 'origin/main', {
    size: 1.61 * GB, has_ignored_files: true,
  }),
  worktree('storefront-web', 'cart-rounding', 'fix/cart-rounding', 'origin/development', {
    size: 1.48 * GB, state: 'squashed', junk_file_count: 2,
  }),
  worktree('api-gateway', 'rate-limits', 'feature/rate-limits', 'origin/develop', {
    size: 438 * MB, state: 'squashed',
  }),
  worktree('mobile-app', 'release-2-4', 'release/2.4', 'origin/main', {
    size: 305 * MB, is_detached: true,
  }),
  worktree('storefront-web', 'search-filters', 'feature/search-filters', 'origin/main', {
    size: 1.52 * GB, is_dirty: true,
  }),
  worktree('mobile-app', 'offline-sync', 'spike/offline-sync', 'origin/main', {
    size: 297 * MB, is_locked: true, lock_reason: 'on external drive',
  }),
  worktree('api-gateway', 'old-auth', 'chore/old-auth', '', {
    state: 'stale', stale_reason: 'directory not found',
  }),
];

const worktreeDiagnostics = [
  `${PROJECTS}/storefront-web: compared against origin/HEAD, origin/main, origin/development`,
  `${PROJECTS}/api-gateway: compared against origin/HEAD, origin/main, origin/develop`,
  `${PROJECTS}/mobile-app: compared against origin/HEAD, origin/main`,
];

const now = Math.floor(Date.now() / 1000);

function cache(id, kind, label, path, reclaimable, extra = {}) {
  return {
    id,
    kind,
    label,
    path,
    logical_size: reclaimable,
    allocated_size: reclaimable,
    reclaimable_size: reclaimable,
    last_modified: now - 3 * DAY,
    safety: 'safe',
    cleanup: { type: 'remove_dir' },
    note: null,
    ...extra,
  };
}

const external = (program, args) => ({
  type: 'external_command', program, args, display: `${program} ${args.join(' ')}`,
});

const caches = [
  cache('pnpm-store-orphaned', 'orphaned_store', '~/.pnpm-store (abandoned store)', `${HOME}/.pnpm-store`, 6.8 * GB, {
    logical_size: 9.4 * GB,
    last_modified: now - 210 * DAY,
    note: 'Left behind after pnpm moved its default store location. Deleting a store never breaks an installed project: files in node_modules are hardlinks, so the data survives as long as a link points at it.',
  }),
  cache('npm-cacache', 'package_manager', '.npm/_cacache', `${HOME}/.npm/_cacache`, 4.2 * GB, {
    note: 'npm content cache — rebuilt on next install',
  }),
  cache('pnpm-store-active', 'package_manager', '~/Library/pnpm/store (active store)', `${HOME}/Library/pnpm/store`, 0, {
    allocated_size: 11.3 * GB,
    logical_size: 11.3 * GB,
    last_modified: now - 1 * DAY,
    cleanup: external('pnpm', ['store', 'prune']),
    note: 'Active store — pruned rather than deleted, so packages still referenced by installed projects survive.',
  }),
  cache('bun-install-cache', 'package_manager', '.bun/install/cache', `${HOME}/.bun/install/cache`, 1.9 * GB, {
    note: 'bun package cache — re-downloaded on next install',
  }),
  cache('uv-cache', 'package_manager', '.cache/uv', `${HOME}/.cache/uv`, 0, {
    allocated_size: 2.6 * GB,
    logical_size: 2.6 * GB,
    cleanup: external('uv', ['cache', 'prune']),
    note: 'Pruned by uv itself, which keeps entries still referenced by environments.',
  }),
  cache('npm-npx', 'package_manager', '.npm/_npx', `${HOME}/.npm/_npx`, 780 * MB, {
    last_modified: now - 45 * DAY,
    note: 'one-off packages fetched by npx; nothing depends on them',
  }),
  cache('puppeteer-old-build', 'runtime', '.cache/puppeteer/chrome/mac_arm-127.0.6533.88', `${HOME}/.cache/puppeteer/chrome/mac_arm-127.0.6533.88`, 402 * MB, {
    last_modified: now - 120 * DAY,
    note: 'Superseded browser build; the newest one is kept.',
  }),
  cache('nvm-old-node', 'runtime', 'nvm v18.20.4', `${HOME}/.nvm/versions/node/v18.20.4`, 196 * MB, {
    safety: 'needs_review',
    last_modified: now - 300 * DAY,
    note: 'Node v18.20.4 and its globally installed packages. Reinstall with `nvm install v18.20.4` if needed — globals do not come back with it.',
  }),
  cache('unrotated-log', 'log', '/opt/homebrew/var/log/postgresql@16.log', '/opt/homebrew/var/log/postgresql@16.log', 1.1 * GB, {
    cleanup: { type: 'truncate_file' },
    note: 'Emptied in place rather than deleted — the service holds this file open, and removing it would keep the space locked until a restart.',
  }),
];

const settings = {
  hide_dock: true,
  watched_folders: [`${PROJECTS}`, '/Volumes/Work/clients'],
};

const ticks = [2.9, 1.4, 1.1, 0.9, 1.0, 0.8].map((seconds, index) => ({
  duration_ms: Math.round(seconds * 1000),
  git_spawns: index === 0 ? 24 : 6,
  cache_hits: index === 0 ? 0 : 5,
  cache_considered: 6,
  removable_worktrees: 4,
}));

const fixtures = {
  scanPath: PROJECTS,
  nodeModules,
  worktrees,
  worktreeDiagnostics,
  caches,
  settings,
  ticks,
};

// ---------------------------------------------------------------------------
// The fake Tauri bridge, injected before the app's own scripts run.
// ---------------------------------------------------------------------------

function installTauriMock(data) {
  const wait = (ms) => new Promise((done) => setTimeout(done, ms));
  let current = structuredClone(data.settings);
  const sum = (items, key) => items.reduce((total, item) => total + item[key], 0);

  const handlers = {
    'plugin:dialog|open': () => data.scanPath,
    scan_for_node_modules: async ({ path }) => {
      await wait(150);
      return {
        folders: data.nodeModules,
        total_size: sum(data.nodeModules, 'size'),
        total_allocated_size: sum(data.nodeModules, 'allocated_size'),
        total_reclaimable_size: sum(data.nodeModules, 'reclaimable_size'),
        scan_path: path,
      };
    },
    scan_for_merged_worktrees: async ({ path }) => {
      await wait(150);
      return {
        worktrees: data.worktrees,
        total_size: sum(data.worktrees.filter((w) => !w.is_dirty && !w.is_locked), 'size'),
        scan_path: path,
        warnings: [],
        diagnostics: data.worktreeDiagnostics,
      };
    },
    scan_for_dev_caches: async () => {
      await wait(150);
      return {
        targets: data.caches,
        total_reclaimable_size: sum(data.caches, 'reclaimable_size'),
        warnings: [],
      };
    },
    get_settings: () => current,
    dock_toggle_available: () => true,
    set_hide_dock: ({ hidden }) => (current = { ...current, hide_dock: hidden }),
    add_watched_folder: ({ folder }) => (current = {
      ...current, watched_folders: [...current.watched_folders, folder],
    }),
    remove_watched_folder: ({ folder }) => (current = {
      ...current, watched_folders: current.watched_folders.filter((f) => f !== folder),
    }),
    tick_diagnostics: () => data.ticks,
    refresh_tray_now: () => null,
  };

  let callbackId = 0;
  window.__TAURI_INTERNALS__ = {
    metadata: { currentWindow: { label: 'main' }, currentWebview: { windowLabel: 'main', label: 'main' } },
    invoke: async (cmd, args = {}) => {
      const handler = handlers[cmd];
      if (!handler) throw new Error(`screenshots: no fixture for command "${cmd}"`);
      return handler(args);
    },
    transformCallback: (callback) => {
      const id = ++callbackId;
      window[`_${id}`] = callback;
      return id;
    },
    unregisterCallback: (id) => delete window[`_${id}`],
    convertFileSrc: (path) => path,
  };
}

// ---------------------------------------------------------------------------
// Capture
// ---------------------------------------------------------------------------

const hasCwebp = spawnSync('cwebp', ['-version']).status === 0;
const written = [];

async function save(page, name, options = {}) {
  const png = join(tmpDir, `${name}.png`);
  await page.screenshot({ path: png, ...options });
  let file;
  if (hasCwebp) {
    file = join(outDir, `${name}.webp`);
    execFileSync('cwebp', ['-quiet', '-q', '82', '-m', '6', png, '-o', file]);
  } else {
    file = join(outDir, `${name}.png`);
    execFileSync('cp', [png, file]);
  }
  written.push(file);
}

async function main() {
  rmSync(tmpDir, { recursive: true, force: true });
  mkdirSync(tmpDir, { recursive: true });
  mkdirSync(outDir, { recursive: true });

  const server = await createServer({
    root,
    configFile: join(root, 'vite.config.ts'),
    server: { port: 5199, strictPort: false },
    logLevel: 'warn',
  });
  await server.listen();
  const url = server.resolvedUrls.local[0];

  const browser = await chromium.launch();
  try {
    const context = await browser.newContext({
      viewport: { width: 1440, height: 900 },
      deviceScaleFactor: 1,
      colorScheme: 'light',
      locale: 'en-US',
    });
    await context.addInitScript(installTauriMock, fixtures);
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', (error) => errors.push(error.message));
    page.on('console', (message) => {
      if (message.type() === 'error') errors.push(message.text());
    });

    await page.goto(url, { waitUntil: 'networkidle' });
    await page.getByText('No folder selected').waitFor();
    await save(page, 'empty-desktop');

    // Scan: the folder picker resolves instantly to the fixture path.
    await page.getByRole('button', { name: 'Select Folder' }).click();
    await page.getByText('Merged Git worktrees').waitFor();
    await page.getByRole('button', { name: 'Scan caches' }).click();
    await page.getByText('Abandoned stores').waitFor();

    // A realistic selection so the footer with "Space to free" appears.
    for (const project of ['storefront-web', 'mobile-app', 'api-gateway', 'legacy-crm']) {
      await page.getByLabel(`Select node_modules in ${project}`).check();
    }
    await page.getByRole('button', { name: 'Select removable' }).click();
    // Checking boxes scrolls them into view; the overview shot starts from the top.
    const scroller = page.locator('main');
    await scroller.evaluate((element) => { element.scrollTop = 0; });
    await page.mouse.move(0, 0);
    await save(page, 'overview-desktop');

    // Open-graph preview at 1200x630 — the app's own minimum width, so no column is cut off.
    await page.setViewportSize({ width: 1200, height: 630 });
    await page.screenshot({ path: join(root, 'site', 'assets', 'og-image.png') });
    await page.setViewportSize({ width: 1440, height: 900 });
    written.push(join(root, 'site', 'assets', 'og-image.png'));

    const scrollTo = async (text) => {
      await page.getByText(text, { exact: true }).first().evaluate((element) => {
        const section = element.closest('section');
        const scroller = document.querySelector('main');
        scroller.scrollTop += section.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 24;
      });
    };

    await scrollTo('Merged Git worktrees');
    await page.getByText('Base branches compared').click();
    await save(page, 'worktrees-desktop');

    await scrollTo('Developer caches');
    await page.getByRole('button', { name: 'Select safe' }).click();
    await page.mouse.move(0, 0);
    await save(page, 'caches-desktop');

    await scroller.evaluate((element) => { element.scrollTop = 0; });
    await page.getByRole('button', { name: 'Remove Selected' }).click();
    await page.getByText('Remove selected items?').waitFor();
    await save(page, 'confirm-desktop');
    await page.getByRole('button', { name: 'Cancel' }).click();

    await page.getByRole('button', { name: 'Settings' }).click();
    await page.getByText('Background refresh').waitFor();
    await page.getByText('Last run took').waitFor();
    await save(page, 'settings-desktop');

    if (errors.length > 0) {
      throw new Error(`The app logged errors while capturing:\n${errors.join('\n')}`);
    }
  } finally {
    await browser.close();
    await server.close();
  }

  for (const file of written) {
    const kb = Math.round(statSync(file).size / 1024);
    const flag = kb > 300 ? '  <-- over 300 KB' : '';
    console.log(`${file.replace(`${root}/`, '')}  ${kb} KB${flag}`);
  }
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
