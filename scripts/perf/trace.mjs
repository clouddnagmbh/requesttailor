// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand A: `trace_inject` (perf/README.md): `trace::inject` with and without an
// existing header, 100 000 calls, release build, median per call
// (crates/rt-core/tests/trace_perf.rs).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureTrace({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-trace-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-core", "--test", "trace_perf", "--", "trace_inject_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "trace.json"));
    return { results: { trace_inject: r.trace_inject }, details: { fresh_ms: r.fresh_ms, existing_ms: r.existing_ms, off_ms: r.off_ms } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
