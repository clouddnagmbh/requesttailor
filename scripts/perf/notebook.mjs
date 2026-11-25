// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand W: `notebook_open` (perf/README.md). Reading a notebook of 50 request blocks
// and writing it back, median of 9, release build of rt-app (crates/rt-app/tests/notebook_perf.rs).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureNotebook({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-notebook-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "notebook_perf", "--", "notebook_open_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "notebook.json"));
    return { results: { notebook_open: r.notebook_open }, details: { worst_ms: r.worst, blocks: 50 } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
