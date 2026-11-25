// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand D: `access_check` (perf/README.md). The own time of one two-user access check
// (two profiles, a fake transport that answers at once, comparison and evidence of 2 KB JSON
// bodies), median of 200 checks, release build of rt-app (crates/rt-app/tests/access_perf.rs).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureAccess({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-access-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "access_perf", "--", "access_check_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "access.json"));
    return {
      results: { access_check: r.access_check },
      details: { p95_ms: r.p95, checks: 200 },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
