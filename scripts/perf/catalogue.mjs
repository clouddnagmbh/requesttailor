// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I: `catalogue_index` (perf/README.md). Reading and searching a plugin index with
// 1000 entries, release build of rt-app, median of 9 (crates/rt-app/tests/catalogue_perf.rs,
// `catalogue_index_is_measured`); the SHA-256 of a 10 MiB module is reported in the details and
// is held at 250 ms (software SHA-256).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureCatalogue({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-catalogue-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "catalogue_perf", "--", "catalogue_index_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "catalogue.json"));
    return {
      results: { catalogue_index: r.catalogue_index },
      details: { runs: r.runs, sha256_10_mib_ms: r.sha256_10_mib_ms, index_bytes: r.bytes },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
