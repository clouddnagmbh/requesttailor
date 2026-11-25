// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C: `journal_overhead`, the time the test-data journal adds to one writing send
// (perf/README.md). `crates/rt-app/tests/journal_perf.rs` hands 10 000 exchanges to the journal
// (file storage, background writer) in a release build, times each hand-over, and reports the
// median; it also reads 5000 entries back (budget: under 50 ms, asserted in the test).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureJournal({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-journal-"));
  try {
    run(
      "cargo",
      ["test", "--release", "--locked", "-p", "rt-app", "--test", "journal_perf", "--", "journal_overhead_is_measured", "--exact", "--nocapture"],
      { cwd: root, env: { ...process.env, RT_PERF_OUT: out } },
    );
    const r = readJson(join(out, "journal.json"));
    return {
      results: { journal_overhead: r.journal_overhead },
      details: { entries: r.entries, p99_ms: r.p99_ms, read_5000_ms: r.read_5000_ms },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
