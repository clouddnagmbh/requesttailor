// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: `view_apply` (perf/README.md). A saved JSONPath view and a table on a
// 10 MiB JSON body kept by the `ResponseStore`, release build of rt-app, median of 9, the slower
// of the two (crates/rt-app/tests/views.rs, `view_apply_is_measured`); the same table at the
// 32 MiB limit is reported in the details and must stay under 500 ms.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureViews({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-views-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "views", "--", "view_apply_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "views.json"));
    return {
      results: { view_apply: r.view_apply },
      details: { json_runs: r.json_runs, table_runs: r.table_runs, at_32_mib_ms: r.at_32_mib },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
