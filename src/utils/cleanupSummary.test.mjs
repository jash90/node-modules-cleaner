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
