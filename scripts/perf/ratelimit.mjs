// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand D: `ratelimit_wait` (perf/README.md): a response says `Retry-After: 1`; the
// runner with `--respect-limits` sends again one second later. The error of that wait, median
// of 9 real runs (crates/rt-core/tests/ratelimit.rs, release build, real sleeps).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureRateLimit({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-ratelimit-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-core", "--test", "ratelimit", "--", "ratelimit_wait_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "ratelimit.json"));
    return { results: { ratelimit_wait: r.ratelimit_wait }, details: { errors_ms: r.errors_ms } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
