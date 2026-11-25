// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: `debug_step` (perf/README.md): one step of the script debugger, from one
// pause to the next, release build, median of 100 (crates/rt-script/tests/debug.rs).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureDebugger({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-debugger-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-script", "--test", "debug", "--", "debug_step_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "debugger.json"));
    return { results: { debug_step: r.debug_step }, details: { max_ms: r.max_ms } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
