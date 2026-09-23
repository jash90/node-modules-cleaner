import assert from "node:assert/strict";
import test from "node:test";

import * as cleanupSummary from "./cleanupSummary.ts";
import {
  createCleanupSummary,
  runCleanupScans,
} from "./cleanupSummary.ts";

test("combines selected node_modules and worktrees into one summary", () => {
  assert.deepEqual(
    createCleanupSummary({
      nodeModules: [
        { path: "/projects/a/node_modules", size: 500 },
        { path: "/projects/b/node_modules", size: 500 },
        { path: "/projects/c/node_modules", size: 500 },
      ],
      worktrees: [
        { path: "/worktrees/a", size: 1_000 },
        { path: "/worktrees/b", size: 1_500 },
      ],
    }),
    {
      totalCount: 5,
      totalSize: 4_000,
      items: [
        { label: "node_modules folders", count: 3 },
        { label: "merged Git worktrees", count: 2 },
      ],
      hasEstimate: false,
    },
  );
});

test("omits empty categories from the confirmation breakdown", () => {
  assert.deepEqual(
    createCleanupSummary({
      nodeModules: [],
      worktrees: [
        { path: "/worktrees/a", size: 1_000 },
        { path: "/worktrees/b", size: 1_500 },
      ],
    }).items,
    [{ label: "merged Git worktrees", count: 2 }],
  );
});

test("does not double-count node_modules inside a selected worktree", () => {
  assert.deepEqual(
    createCleanupSummary({
      nodeModules: [
        { path: "/projects/repo-worktree/node_modules", size: 400 },
        { path: "/projects/standalone/node_modules", size: 100 },
      ],
      worktrees: [
        { path: "/projects/repo-worktree", size: 1_000 },
      ],
    }),
    {
      totalCount: 3,
      totalSize: 1_100,
      items: [
        { label: "node_modules folders", count: 2 },
        { label: "merged Git worktrees", count: 1 },
      ],
      hasEstimate: false,
    },
  );
});

test("reduces cached parent worktree sizes after nested folders are deleted", () => {
  assert.equal(typeof cleanupSummary.adjustWorktreeSizes, "function");

  assert.deepEqual(
    cleanupSummary.adjustWorktreeSizes(
      [
        { path: "/worktrees/feature", size: 1_000 },
        { path: "/worktrees/unrelated", size: 700 },
      ],
      [
        { path: "/worktrees/feature/node_modules", size: 300 },
        { path: "/outside/node_modules", size: 200 },
      ],
    ),
    [
      { path: "/worktrees/feature", size: 700 },
      { path: "/worktrees/unrelated", size: 700 },
    ],
  );
});

test("removes node_modules records nested in deleted worktrees", () => {
  assert.equal(typeof cleanupSummary.removeCandidatesWithinPaths, "function");

  assert.deepEqual(
    cleanupSummary.removeCandidatesWithinPaths(
      [
        { path: "/worktrees/removed/node_modules", size: 300 },
        { path: "/worktrees/kept/node_modules", size: 200 },
      ],
      ["/worktrees/removed"],
    ),
    [{ path: "/worktrees/kept/node_modules", size: 200 }],
  );
});

test("finishes node_modules cleanup before adjusting and removing worktrees", async () => {
  assert.equal(typeof cleanupSummary.runCleanupDeletion, "function");

  const events = [];
  await cleanupSummary.runCleanupDeletion(
    async () => {
      events.push("delete node_modules");
      return [{ path: "/worktree/node_modules", size: 25 }];
    },
    (deletedFolders) => {
      events.push(`adjust ${deletedFolders[0].size}`);
    },
    async () => {
      events.push("delete worktrees");
      return ["/worktree"];
    },
    (removedWorktreePaths) => {
      events.push(`reconcile ${removedWorktreePaths[0]}`);
    },
  );

  assert.deepEqual(events, [
    "delete node_modules",
    "adjust 25",
    "delete worktrees",
    "reconcile /worktree",
  ]);
});

test("runs both scans for the same path even when one fails", async () => {
  const calls = [];
  const results = await runCleanupScans(
    "/tmp/project",
    async (path) => {
      calls.push(["node_modules", path]);
      throw new Error("node_modules scan failed");
    },
    async (path) => {
      calls.push(["worktrees", path]);
    },
  );

  assert.deepEqual(calls, [
    ["node_modules", "/tmp/project"],
    ["worktrees", "/tmp/project"],
  ]);
  assert.equal(results[0].status, "rejected");
  assert.equal(results[1].status, "fulfilled");
});

test("only a lock keeps a worktree from being selected", () => {
  assert.equal(cleanupSummary.isSelectableWorktree({ is_locked: false, is_dirty: true }), true);
  assert.equal(cleanupSummary.isSelectableWorktree({ is_locked: true, is_dirty: false }), false);
});

test("counts removable worktrees the way the menu bar does", () => {
  const worktrees = [
    { is_locked: false, is_dirty: false, state: "merged" },
    { is_locked: false, is_dirty: true, state: "squashed" },
    { is_locked: true, is_dirty: false, state: "merged" },
    { is_locked: false, is_dirty: false, state: "stale" },
  ];

  assert.equal(cleanupSummary.countRemovableWorktrees(worktrees), 2);
});

test("asks for consent only for selected worktrees that hold uncommitted changes", () => {
  const worktrees = [
    { path: "/wt/clean", is_dirty: false },
    { path: "/wt/dirty", is_dirty: true },
  ];

  assert.deepEqual(
    cleanupSummary.worktreesNeedingConsent(worktrees).map((worktree) => worktree.path),
    ["/wt/dirty"],
  );
});

test("finds the menu bar count for the scanned folder, ignoring a trailing slash", () => {
  const stats = {
    removable_worktrees: 5,
    folders: [
      { folder: "/Users/me/Projects/", removable_worktrees: 3 },
      { folder: "/Users/me/Other", removable_worktrees: 2 },
    ],
  };

  assert.equal(cleanupSummary.trayCountFor(stats, "/Users/me/Projects"), 3);
  assert.equal(cleanupSummary.trayCountFor(stats, "/Users/me/Elsewhere"), null);
});

test("flags a stale list only when a fresh menu bar count disagrees after a good scan", () => {
  const stats = { removable_worktrees: 1, folders: [{ folder: "/p", removable_worktrees: 1 }] };
  const base = {
    stats,
    statsReceivedAt: 20,
    listBuiltAt: 10,
    scanPath: "/p",
    windowCount: 2,
    isBusy: false,
    scanFailed: false,
  };

  assert.deepEqual(cleanupSummary.detectTrayMismatch(base), { trayCount: 1, windowCount: 2 });
  assert.equal(cleanupSummary.detectTrayMismatch({ ...base, windowCount: 1 }), null);
  // A count from before the list was rebuilt describes a different moment.
  assert.equal(cleanupSummary.detectTrayMismatch({ ...base, statsReceivedAt: 5 }), null);
  assert.equal(cleanupSummary.detectTrayMismatch({ ...base, isBusy: true }), null);
  // A failed scan leaves an empty list; that is an error, not a sign that worktrees changed.
  assert.equal(cleanupSummary.detectTrayMismatch({ ...base, scanFailed: true }), null);
  assert.equal(cleanupSummary.detectTrayMismatch({ ...base, stats: null }), null);
});

test("adds selected developer caches to the one confirmation summary", () => {
  assert.deepEqual(
    createCleanupSummary({
      nodeModules: [{ path: "/p/a/node_modules", size: 100 }],
      worktrees: [{ path: "/wt/a", size: 1_000 }],
      caches: [
        { path: "/Users/me/.npm/_cacache", size: 300 },
        { path: "/Users/me/.gradle/caches", size: 50 },
      ],
    }),
    {
      totalCount: 4,
      totalSize: 1_450,
      items: [
        { label: "node_modules folders", count: 1 },
        { label: "merged Git worktrees", count: 1 },
        { label: "developer caches", count: 2 },
      ],
      hasEstimate: false,
    },
  );
});

test("marks the total as a floor when a selected cache is handed to a prune command", () => {
  const summary = createCleanupSummary({
    nodeModules: [],
    worktrees: [],
    caches: [{ path: "/Users/me/.pnpm-store", size: 10, isEstimate: true }],
  });

  assert.equal(summary.hasEstimate, true);
  assert.equal(summary.totalCount, 1);
});

test("lists every failing path with its reason and the sudo hint once", () => {
  const message = cleanupSummary.describeFailures("node_modules folder", [
    { path: "/p/a/node_modules", reason: "Permission denied", sudoHint: "sudo chown -R me /p" },
    { path: "/p/b/node_modules", reason: "Not a node_modules folder", sudoHint: "sudo chown -R me /p" },
  ]);

  assert.equal(
    message,
    [
      "Failed to remove 2 node_modules folder(s):",
      "/p/a/node_modules: Permission denied",
      "/p/b/node_modules: Not a node_modules folder",
      "Try: sudo chown -R me /p",
    ].join("\n"),
  );
  assert.equal(cleanupSummary.describeFailures("worktree", []), null);
});

test("names the first file a node_modules delete left behind and how many more", () => {
  assert.deepEqual(
    cleanupSummary.nodeModulesFailures([
      { path: "/p/ok/node_modules", success: true, removed_bytes: 5, error: null, failed_paths: [], sudo_hint: null },
      {
        path: "/p/a/node_modules",
        success: false,
        removed_bytes: 1,
        error: "3 item(s) could not be deleted",
        failed_paths: [
          { path: "/p/a/node_modules/x/y", reason: "Permission denied" },
          { path: "/p/a/node_modules/x/z", reason: "Permission denied" },
          { path: "/p/a/node_modules/w", reason: "Permission denied" },
        ],
        sudo_hint: "sudo rm -rf /p/a/node_modules",
      },
      { path: "/etc", success: false, removed_bytes: 0, error: "Not a node_modules folder", failed_paths: [], sudo_hint: null },
    ]),
    [
      {
        path: "/p/a/node_modules",
        reason: "/p/a/node_modules/x/y: Permission denied (+2 more)",
        sudoHint: "sudo rm -rf /p/a/node_modules",
      },
      { path: "/etc", reason: "Not a node_modules folder", sudoHint: null },
    ],
  );
});

test("drops worktrees that stopped being worktrees and blocks those that stopped being safe", () => {
  assert.equal(cleanupSummary.classifyWorktreeFailure("No longer registered"), "gone");
  assert.equal(
    cleanupSummary.classifyWorktreeFailure("No longer merged into the repository's base branches"),
    "blocked",
  );
  assert.equal(
    cleanupSummary.classifyWorktreeFailure("Changed since scan — someone committed here"),
    "blocked",
  );
  assert.equal(cleanupSummary.classifyWorktreeFailure("Permission denied"), "retry");
  assert.equal(cleanupSummary.classifyWorktreeFailure(null), "retry");

  const { kept, blocked } = cleanupSummary.applyWorktreeFailures(
    [{ path: "/wt/gone" }, { path: "/wt/moved-on" }, { path: "/wt/locked-fs" }, { path: "/wt/other" }],
    [
      { path: "/wt/gone", success: false, error: "No longer registered", already_gone: false },
      { path: "/wt/moved-on", success: false, error: "Changed since scan — someone committed here", already_gone: false },
      { path: "/wt/locked-fs", success: false, error: "Permission denied", already_gone: false },
    ],
  );

  assert.deepEqual(kept.map((worktree) => worktree.path), ["/wt/moved-on", "/wt/locked-fs", "/wt/other"]);
  assert.deepEqual([...blocked], [["/wt/moved-on", "Changed since scan — someone committed here"]]);
});

test("keeps pruned cache rows, whose directory is still there, and drops deleted ones", () => {
  const targets = [
    { path: "/c/npm", cleanup: { type: "remove_dir" } },
    { path: "/c/pnpm", cleanup: { type: "external_command" } },
    { path: "/c/log", cleanup: { type: "truncate_file" } },
    { path: "/c/failed", cleanup: { type: "remove_dir" } },
  ];
  const results = [
    { path: "/c/npm", success: true },
    { path: "/c/pnpm", success: true },
    { path: "/c/log", success: true },
    { path: "/c/failed", success: false },
  ];

  const { remaining, prunedPaths } = cleanupSummary.applyCacheCleanResults(targets, results);

  assert.deepEqual(remaining.map((target) => target.path), ["/c/pnpm", "/c/failed"]);
  assert.deepEqual(prunedPaths, ["/c/pnpm"]);
});

test("a cache failure does not stop node_modules and worktrees, and is reported", async () => {
  const calls = [];
  const report = await cleanupSummary.runUnifiedCleanup(
    async () => {
      calls.push("chain");
      return {
        nodeModules: { removed: 3, failed: 1, freedBytes: 900 },
        worktrees: { removed: 1, failed: 0, freedBytes: 0 },
      };
    },
    async () => {
      calls.push("caches");
      throw new Error("clean_dev_caches crashed");
    },
    { nodeModules: 4, worktrees: 1, caches: 2 },
  );

  assert.deepEqual(calls.sort(), ["caches", "chain"]);
  assert.deepEqual(report.nodeModules, { removed: 3, failed: 1, freedBytes: 900, messages: [] });
  assert.deepEqual(report.worktrees, { removed: 1, failed: 0, freedBytes: 0, messages: [] });
  assert.deepEqual(report.caches, {
    removed: 0,
    failed: 2,
    freedBytes: 0,
    messages: ["Error: clean_dev_caches crashed"],
  });
});

test("a node_modules/worktree failure does not stop caches", async () => {
  let cachesRan = false;
  const report = await cleanupSummary.runUnifiedCleanup(
    async () => {
      throw new Error("delete_folders crashed");
    },
    async () => {
      cachesRan = true;
      return { removed: 2, failed: 0, freedBytes: 700 };
    },
    { nodeModules: 2, worktrees: 1, caches: 2 },
  );

  assert.equal(cachesRan, true);
  assert.deepEqual(report.caches, { removed: 2, failed: 0, freedBytes: 700, messages: [] });
  assert.equal(report.nodeModules.failed, 2);
  assert.equal(report.worktrees.failed, 1);
  assert.deepEqual(report.nodeModules.messages, ["Error: delete_folders crashed"]);
});

test("reports each attempted category on its own line", () => {
  const empty = { removed: 0, failed: 0, freedBytes: 0, messages: [] };
  assert.deepEqual(
    cleanupSummary.formatCleanupReport({
      nodeModules: { ...empty, removed: 3, failed: 1, freedBytes: 1_024 },
      worktrees: empty,
      caches: { ...empty, failed: 2, messages: ["Error: crashed"] },
    }, (bytes) => `${bytes} B`),
    [
      "node_modules: removed 3 · failed 1 · freed 1024 B",
      "developer caches: removed 0 · failed 2 — Error: crashed",
    ],
  );
});

test("counts what was not confirmed removed as failed, and frees only reported bytes", () => {
  assert.deepEqual(
    cleanupSummary.countOutcome(3, [
      { success: true, removed_bytes: 100 },
      { success: false, removed_bytes: 20 },
    ]),
    { removed: 1, failed: 2, freedBytes: 120 },
  );
  // A call that threw returns no results: every requested item is a failure.
  assert.deepEqual(cleanupSummary.countOutcome(2, []), { removed: 0, failed: 2, freedBytes: 0 });
  assert.deepEqual(
    cleanupSummary.countOutcome(1, [{ success: true }]),
    { removed: 1, failed: 0, freedBytes: 0 },
  );
});
