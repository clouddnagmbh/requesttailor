// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand F: `lint_1000` (perf/README.md). The four rules and the health score on a
// collection of 1000 requests (variables everywhere, 30 % with scripts, 50 variables), model in
// memory, release build of rt-app, median of 9
// (crates/rt-app/tests/lint.rs, `lint_of_1000_requests_is_measured`).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureLint({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-lint-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "lint", "--", "lint_of_1000_requests_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "lint.json"));
    return { results: { lint_1000: r.lint_1000 }, details: { runs: r.runs } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
