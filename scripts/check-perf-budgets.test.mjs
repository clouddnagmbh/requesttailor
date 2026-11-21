// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { evaluate, parseBudgets } from "./check-perf-budgets.mjs";

const budgets = parseBudgets(readFileSync(new URL("../perf/budgets.toml", import.meta.url), "utf8"));

test("budgets.toml carries the product budgets", () => {
  assert.equal(budgets.cold_start.max, 500);
  assert.equal(budgets.idle_memory.max, 150);
  assert.equal(budgets.cold_start.unit, "ms");
});

test("measurements at or under budget pass, over budget fails", () => {
  const rows = evaluate(budgets, { cold_start: 500, idle_memory: 151 });
  const status = Object.fromEntries(rows.map((r) => [r.name, r.status]));
  assert.equal(status.cold_start, "ok");
  assert.equal(status.idle_memory, "over");
  assert.equal(status.large_response_frame, "not measured");
});

test("results for metrics without a budget are rejected", () => {
  assert.throws(() => evaluate(budgets, { startup: 1 }), /unknown metrics/);
});

test("a metric without a numeric max is rejected", () => {
  assert.throws(() => parseBudgets('[metrics.x]\nunit = "ms"\n'), /numeric max/);
  assert.throws(() => parseBudgets("[metrics.x]\nmax = fast\n"), /not a number/);
});
