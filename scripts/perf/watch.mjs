// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: `watch_resend`, the time from a changed file to the request on the wire
// for `rt send --watch` (perf/README.md). `crates/rt-cli/tests/watch.rs` starts the release
// `rt`, changes the request file nine times and reads the arrival times at a local server;
// the median is the result.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureWatch({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-watch-"));
  try {
    run(
      "cargo",
      ["test", "--release", "--locked", "-p", "rt-cli", "--test", "watch", "--", "watch_resend_is_measured", "--exact", "--nocapture"],
      { cwd: root, env: { ...process.env, RT_PERF_OUT: out } },
    );
    const r = readJson(join(out, "watch.json"));
    return { results: { watch_resend: r.watch_resend }, details: { samples_ms: r.samples } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
