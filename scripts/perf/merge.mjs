// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Merges the results of several machines (CI: one per OS) into one results file with the worst
// value per metric (every budget is a maximum), so the gate holds on every OS.
// Usage: node scripts/perf/merge.mjs <out.json> <results.json>...

import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

/** [{ metric: number }] → { metric: worst (largest) value }. Non-numbers are rejected. */
export function mergeWorst(all) {
  const out = {};
  for (const results of all) {
    for (const [k, v] of Object.entries(results)) {
      if (typeof v !== "number" || !Number.isFinite(v)) throw new Error(`metric ${k}: ${JSON.stringify(v)} is not a number`);
      out[k] = k in out ? Math.max(out[k], v) : v;
    }
  }
  return out;
}

function main() {
  const [out, ...inputs] = process.argv.slice(2);
  if (!out || inputs.length === 0) {
    console.error("usage: node scripts/perf/merge.mjs <out.json> <results.json>...");
    process.exit(2);
  }
  const merged = mergeWorst(inputs.map((f) => JSON.parse(readFileSync(f, "utf8"))));
  writeFileSync(out, `${JSON.stringify(merged, null, 2)}\n`);
  for (const f of inputs) console.log(`${f}: ${readFileSync(f, "utf8").replace(/\s+/g, " ").trim()}`);
  console.log(`merged (worst per metric): ${JSON.stringify(merged)}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) main();
