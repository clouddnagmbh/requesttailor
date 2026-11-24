// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: `env_compare` (perf/README.md): 100 reading requests in two environments
// with a fake transport, every answer different; the run, the comparison of the steps and the
// result list (crates/rt-app/tests/envcompare.rs, release build), median of 9.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureEnvCompare({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-envcompare-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "envcompare", "--", "env_compare_of_100_requests_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "envcompare.json"));
    return { results: { env_compare: r.env_compare }, details: { runs: r.runs, requests: 100, environments: 2 } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
