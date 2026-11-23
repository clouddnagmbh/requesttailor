// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: measures the commit graph (budgets `git_graph`: first rows of a repository
// with 10 000 commits visible, ms; `git_graph_frame`: p95 frame time while scrolling it, ms).
// Runs apps/web/e2e/perf/graph.measure.ts against the mock backend with a synthetic history of
// 10 000 commits (the view: paging, lane layout, virtualised rows; not Git itself, which
// crates/rt-git/tests/graph.rs times on its own).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** Returns `{ results: { git_graph, git_graph_frame }, details }`. */
export async function measureGraph(ctx) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-graph-"));
  try {
    ctx.run("npx", ["playwright", "test", "--config", "e2e/perf/graph.perf.config.ts"], {
      cwd: ctx.web,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = ctx.readJson(join(out, "graph.json"));
    return {
      results: { git_graph: r.git_graph, git_graph_frame: r.git_graph_frame },
      details: { first_rows: r.first_rows, frames: r.frames, rendered_rows: r.rendered_rows },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
