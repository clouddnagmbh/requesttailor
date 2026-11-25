// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand E: `shrink_probes` (perf/README.md). Probes needed to shrink a request of 200
// parts (100 headers, 99 query parameters, the auth header) with one cause (the budget: at most
// 60) and with three scattered causes (at most 160), and the own time of both runs (under
// 100 ms); a fake transport, release build of rt-app
// (crates/rt-app/tests/shrink.rs, `shrink_probes_are_measured`).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureShrink({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-shrink-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "shrink", "--", "shrink_probes_are_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "shrink.json"));
    return {
      results: { shrink_probes: r.shrink_probes },
      details: { three_causes_probes: r.three_probes, one_cause_ms: r.one_ms, three_causes_ms: r.three_ms },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
