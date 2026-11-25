// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: `egress_overhead`, the time one egress call adds over the transport
// (perf/README.md). `crates/rt-app/tests/egress_perf.rs` makes 1000 calls through a fake
// transport with 100 allowed hosts, a secret header and the guard asked, in a release build,
// and reports the median difference to the bare transport.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureEgress({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-egress-"));
  try {
    run(
      "cargo",
      ["test", "--release", "--locked", "-p", "rt-app", "--test", "egress_perf", "--", "egress_overhead_is_measured", "--exact", "--nocapture"],
      { cwd: root, env: { ...process.env, RT_PERF_OUT: out } },
    );
    const r = readJson(join(out, "egress.json"));
    return {
      results: { egress_overhead: r.egress_overhead },
      details: { calls: r.calls, hosts: r.hosts, median_call_ms: r.median_call_ms, median_transport_ms: r.median_transport_ms },
    };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
