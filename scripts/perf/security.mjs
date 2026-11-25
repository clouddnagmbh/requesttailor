// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand R: `security_passive` (perf/README.md): every passive security rule (with the
// checks of wave 18) over one response with a 100 KB JSON body, three tokens (one with a guessable
// secret) and TLS details (crates/rt-app/tests/security_perf.rs, release build), median of 9.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureSecurity({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-security-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "security_perf", "--", "security_passive_of_one_response_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "security.json"));
    return { results: { security_passive: r.security_passive }, details: { runs: r.runs } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
