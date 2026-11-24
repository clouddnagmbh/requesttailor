// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand B: 1000 stored responses against their snapshots, parsing and JSON diff
// included (crates/rt-app/tests/snapshots_perf.rs, release build), median of 9 (perf/README.md).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureSnapshots({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-snapshots-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "snapshots_perf", "--", "snapshot_compare_of_1000_responses_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "snapshots.json"));
    return { results: { snapshot_compare: r.snapshot_compare }, details: { runs: r.runs } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
