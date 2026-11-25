// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: `sequence_render` (perf/README.md). Two halves:
//  - Rust, release build (crates/rt-app/tests/sequence_perf.rs): the Mermaid text of 500 messages,
//    model from the recorder included, at most 20 ms;
//  - browser (apps/web/e2e/sequence/perf.spec.ts, mock backend, the real view): the SVG layout and
//    first draw of 500 messages at most 100 ms, and the scroll of 2 000 messages between 8
//    participants, p95 frame gap at most 20 ms (asserted by the spec).
// The metric is the worst of the two as a share of its limit, in ms of the 100 ms limit: the
// layout time, or five times the Mermaid time (20 ms ~ 100 ms).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureSequence({ root, web, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-sequence-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "sequence_perf", "--", "sequence_render_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const rust = readJson(join(out, "sequence.json"));
    run("npx", ["playwright", "test", "--config", "e2e/sequence/sequence.perf.config.ts"], {
      cwd: web,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const browser = readJson(join(out, "sequence-browser.json"));
    const value = Math.max(browser.layout_ms, rust.mermaid_ms * 5);
    return {
      results: { sequence_render: Math.round(value * 100) / 100 },
      details: { mermaid_ms: rust.mermaid_ms, mermaid_2000_ms: rust.mermaid_2000_ms, layout_ms: browser.layout_ms, scroll_p95_ms: browser.scroll_p95_ms, scroll_frames: browser.scroll_frames },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
