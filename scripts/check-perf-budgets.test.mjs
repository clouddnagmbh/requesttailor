// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { test } from "node:test";
import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { evaluate, parseBudgets } from "./check-perf-budgets.mjs";

const budgets = parseBudgets(readFileSync(new URL("../perf/budgets.toml", import.meta.url), "utf8"));
const ROOT = fileURLToPath(new URL("..", import.meta.url));
const SCRIPT = fileURLToPath(new URL("./check-perf-budgets.mjs", import.meta.url));

test("budgets.toml carries the product budgets", () => {
  assert.equal(budgets.cold_start.max, 500);
  assert.equal(budgets.idle_memory.max, 150);
  assert.equal(budgets.cold_start.unit, "ms");
  assert.equal(budgets.large_response_frame.max, 20);
  // "Under three keystrokes".
  assert.equal(budgets.send_keystrokes.max, 2);
  // Wave 12: 1000 requests, 100 changed.
  assert.equal(budgets.semantic_diff.max, 200);
  assert.equal(budgets.semantic_diff.unit, "ms");
});

test("measurements at or under budget pass, over budget fails", () => {
  const rows = evaluate(budgets, { cold_start: 500, idle_memory: 151 });
  const status = Object.fromEntries(rows.map((r) => [r.name, r.status]));
  assert.equal(status.cold_start, "ok");
  assert.equal(status.idle_memory, "over");
  assert.equal(status.large_response_frame, "not measured");
  assert.equal(status.send_keystrokes, "not measured");
});

test("results for metrics without a budget are rejected", () => {
  assert.throws(() => evaluate(budgets, { startup: 1 }), /unknown metrics/);
  assert.throws(() => evaluate(budgets, [1]), /must be an object/);
});

test("a metric without a numeric max is rejected", () => {
  assert.throws(() => parseBudgets('[metrics.x]\nunit = "ms"\n'), /numeric max/);
  assert.throws(() => parseBudgets("[metrics.x]\nmax = fast\n"), /not a number/);
});

/** Runs the gate as CI does, on `results` written to a temporary file. */
function gate(results, ...args) {
  const dir = mkdtempSync(join(tmpdir(), "rt-perf-gate-"));
  try {
    const file = join(dir, "results.json");
    writeFileSync(file, JSON.stringify(results));
    return spawnSync(process.execPath, [SCRIPT, file, ...args], { cwd: ROOT, encoding: "utf8" });
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

const ALL_OK = { cold_start: 300, idle_memory: 100, large_response_frame: 16.8, send_keystrokes: 1, semantic_diff: 40, compare_view: 60 };

test("gate: every budget met and measured passes with --require-all", () => {
  const r = gate(ALL_OK, "--require-all");
  assert.equal(r.status, 0, r.stderr);
});

test("gate: an exceeded budget fails the build, with or without --require-all", () => {
  for (const [metric, value] of [
    ["cold_start", 501],
    ["idle_memory", 150.1],
    ["large_response_frame", 33.4],
    ["send_keystrokes", 3],
    ["semantic_diff", 200.1],
  ]) {
    for (const args of [[], ["--require-all"]]) {
      const r = gate({ ...ALL_OK, [metric]: value }, ...args);
      assert.equal(r.status, 1, `${metric}=${value} ${args.join(" ")}`);
      assert.match(r.stderr, new RegExp(`FAILED: ${metric}`));
    }
  }
});

test("gate: a missing measurement fails with --require-all only", () => {
  const { idle_memory: _gone, ...partial } = ALL_OK;
  const strict = gate(partial, "--require-all");
  assert.equal(strict.status, 1);
  assert.match(strict.stderr, /FAILED: idle_memory/);
  assert.equal(gate(partial).status, 0);
  // A value that is not a number counts as missing.
  assert.equal(gate({ ...ALL_OK, cold_start: "fast" }, "--require-all").status, 1);
});

test("gate: no results file at all fails with --require-all", () => {
  const r = spawnSync(process.execPath, [SCRIPT, join(tmpdir(), "rt-perf-none", "results.json"), "--require-all"], {
    cwd: ROOT,
    encoding: "utf8",
  });
  assert.equal(r.status, 1);
});
