// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand H: measures the comparison view (budget `compare_view`: a comparison of 1000
// requests, 100 changed, visible with the answers of both runs, ms). Runs
// apps/web/e2e/perf/compare.measure.ts against the mock backend (the view, not a network);
// median of five comparisons after a warm-up.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** Returns `{ results: { compare_view }, details }`. */
export async function measureCompare(ctx) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-compare-"));
  try {
    ctx.run("npx", ["playwright", "test", "--config", "e2e/perf/compare.perf.config.ts"], {
      cwd: ctx.web,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = ctx.readJson(join(out, "compare.json"));
    return { results: { compare_view: r.compare_view }, details: { first: r.first, runs: r.runs, rendered_rows: r.rendered_rows } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
