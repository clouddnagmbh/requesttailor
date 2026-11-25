// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: `paging_merge`: following and merging 100 pages of 100 items into one stored
// response (crates/rt-app/tests/paging.rs, release build), median of 9 (perf/README.md). The
// details carry the result size and the peak growth of the resident memory while merging.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measurePaging({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-paging-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "paging", "--", "paging_merge_of_100_pages_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "paging.json"));
    return {
      results: { paging_merge: r.paging_merge },
      details: {
        runs: r.runs,
        result_bytes: r.result_bytes,
        peak_growth_bytes: r.peak_growth_bytes,
        peak_over_result: r.result_bytes > 0 ? Number((r.peak_growth_bytes / r.result_bytes).toFixed(2)) : null,
      },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
