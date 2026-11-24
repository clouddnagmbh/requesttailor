// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand A: `observer_overhead`, the time three do-nothing exchange observers add to a
// send against rt-testserver (perf/README.md). `crates/rt-core/tests/observer_perf.rs` sends
// 500 pairs of the same request, with and without the observers, in a release build and
// reports the median difference.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureObserve({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-observe-"));
  try {
    run(
      "cargo",
      ["test", "--release", "--locked", "-p", "rt-core", "--test", "observer_perf", "--", "observer_overhead_is_measured", "--exact", "--nocapture"],
      { cwd: root, env: { ...process.env, RT_PERF_OUT: out } },
    );
    const r = readJson(join(out, "observe.json"));
    return {
      results: { observer_overhead: r.observer_overhead },
      details: { pairs: r.pairs, send_without_ms: r.send_without_ms, send_with_ms: r.send_with_ms },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
