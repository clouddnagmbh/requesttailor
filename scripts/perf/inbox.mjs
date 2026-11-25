// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E: `inbox_latency` (perf/README.md): from the last byte of a request arriving on
// the loopback port to the event at a subscriber (`Inbox::wait`), over 2 000 requests of 2 KB
// (crates/rt-app/tests/inbox_perf.rs, release build). The result is the median; the test also holds
// the 99th percentile to 20 ms and the buffer to its limits (500 requests, 64 MiB).

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureInbox({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-inbox-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-app", "--test", "inbox_perf", "--", "inbox_latency_is_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "inbox.json"));
    return { results: { inbox_latency: r.inbox_latency }, details: { p99_ms: r.p99, requests: r.requests } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
