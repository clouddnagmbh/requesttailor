// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: `response_checks` (perf/README.md). The nine passive rules over the headers
// and the first 1 MiB of a response (error status, so the stack-trace rule reads every line),
// release build of rt-app, median of 9 (crates/rt-app/tests/inspect.rs,
// `response_checks_are_measured`; the same test holds 50 KB to 2 ms and a 100 MB body to the
// cost of a 1 MiB one).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureChecks({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-checks-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "inspect", "--", "response_checks_are_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "checks.json"));
    return { results: { response_checks: r.response_checks }, details: { small_50kb: r.small, huge_100mb: r.huge } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
