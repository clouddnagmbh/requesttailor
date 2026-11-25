// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand H: `schema_changes` (perf/README.md): the changes between two GraphQL schemas of
// 2000 types and 10 000 fields (200 changes) and between two Protobuf versions of 500 messages, in
// memory, with the requests each change affects named (crates/rt-format/tests/
// contract_schemas_perf.rs, release build), median of 9; the slower of the two is the figure.

import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureSchemas({ root, run, readJson }) {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-schemas-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-format", "--test", "contract_schemas_perf", "--", "schema_changes_of_2000_types_and_500_messages_are_measured", "--exact", "--nocapture"], {
      cwd: root,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "schemas.json"));
    return { results: { schema_changes: r.schema_changes }, details: { graphql_ms: r.graphql, protobuf_ms: r.protobuf, runs: r.runs, types: 2000, fields: 10000, messages: 500 } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}
