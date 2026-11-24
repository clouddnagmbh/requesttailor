// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C: the length of the share link of every request of the Bruno corpus, 95th
// percentile (crates/rt-app/tests/share_perf.rs; perf/README.md). The length does not depend on
// the build, so the test runs unoptimized.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureShare({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-share-"));
  try {
    run("cargo", ["test", "--locked", "-p", "rt-app", "--test", "share_perf", "--", "share_url_size_of_the_bruno_corpus_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "share.json"));
    return {
      results: { share_url_size: r.share_url_size },
      details: { median: r.median, max: r.max, links: r.links, refused: r.refused },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
