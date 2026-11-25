// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand F: `consumers_verify` (perf/README.md). The own time of verifying 20
// consumers of 50 requests each (reading, running through the runner against a transport that
// answers at once, text report and JSON), median of 5 runs, release build of rt-app
// (crates/rt-app/tests/consumers_perf.rs).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureConsumers({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-consumers-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "consumers_perf", "--", "consumers_verify_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "consumers.json"));
    return {
      results: { consumers_verify: r.consumers_verify },
      details: { worst_ms: r.worst, consumers: 20, requests_each: 50 },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
