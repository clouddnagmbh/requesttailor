// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Performance gate: compares measurements against perf/budgets.toml.
// Usage: node scripts/check-perf-budgets.mjs [results.json] [--require-all]
// results.json maps metric names to numbers, e.g. { "cold_start": 412 }; scripts/perf/run.mjs
// writes it (perf/README.md). A metric without a number is "not measured". Exceeding a budget
// always fails; with --require-all (CI, since wave 4) a missing measurement fails as well,
// except for a budget marked `pending = "<wave, strand>"` (status "pending": the strand that
// builds its measurement has not landed yet; since wave 15).
// A budget marked `gate = "release"` (cold_start, idle_memory: shared CI runners are too noisy)
// is only reported by default, over its limit or missing; with --release it fails like any other.

import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";

/** Parses the TOML subset used by budgets.toml: [metrics.<name>] tables with string/number values. */
export function parseBudgets(text) {
  const metrics = {};
  let current = null;
  for (const [i, raw] of text.split("\n").entries()) {
    const line = raw.replace(/\s+#.*$/, "").replace(/^#.*$/, "").trim();
    if (line === "") continue;
    const table = line.match(/^\[metrics\.([A-Za-z0-9_]+)\]$/);
    if (table) {
      current = metrics[table[1]] = {};
      continue;
    }
    const kv = line.match(/^([A-Za-z0-9_]+)\s*=\s*(.+)$/);
    if (!kv || current === null) throw new Error(`budgets.toml line ${i + 1}: cannot parse "${raw}"`);
    const value = kv[2].startsWith('"') ? JSON.parse(kv[2]) : Number(kv[2]);
    if (typeof value === "number" && Number.isNaN(value)) {
      throw new Error(`budgets.toml line ${i + 1}: "${kv[2]}" is not a number`);
    }
    current[kv[1]] = value;
  }
  for (const [name, m] of Object.entries(metrics)) {
    if (typeof m.max !== "number") throw new Error(`budgets.toml: metric "${name}" needs a numeric max`);
  }
  return metrics;
}

/** Returns one row per budget: { name, value, max, unit, status: "ok" | "over" | "not measured" |
 *  "pending" } ("pending": not measured, and the budget names the strand that will measure it). */
export function evaluate(budgets, results) {
  if (typeof results !== "object" || results === null || Array.isArray(results)) {
    throw new Error("results must be an object mapping metric names to numbers");
  }
  const unknown = Object.keys(results).filter((k) => !(k in budgets));
  if (unknown.length > 0) throw new Error(`results for unknown metrics: ${unknown.join(", ")}`);
  return Object.entries(budgets).map(([name, b]) => {
    const value = results[name];
    const missing = typeof b.pending === "string" ? "pending" : "not measured";
    const status = typeof value !== "number" ? missing : value <= b.max ? "ok" : "over";
    return { name, value, max: b.max, unit: b.unit ?? "", status, release: b.gate === "release", ...(status === "pending" ? { pending: b.pending } : {}) };
  });
}

function main() {
  const args = process.argv.slice(2);
  const requireAll = args.includes("--require-all");
  const release = args.includes("--release");
  const resultsPath = resolve(args.find((a) => !a.startsWith("--")) ?? "perf/results.json");
  const budgets = parseBudgets(readFileSync(resolve("perf/budgets.toml"), "utf8"));
  const results = existsSync(resultsPath) ? JSON.parse(readFileSync(resultsPath, "utf8")) : {};
  const rows = evaluate(budgets, results);

  for (const r of rows) {
    const shown = r.status === "not measured" || r.status === "pending" ? "-" : `${r.value} ${r.unit}`;
    const note = r.status === "pending" ? `, measured from ${r.pending}` : "";
    console.log(`${r.status.padEnd(12)} ${r.name.padEnd(22)} ${shown.padStart(10)}  (max ${r.max} ${r.unit}${note})`);
  }
  const reportOnly = (r) => r.release && !release;
  for (const r of rows.filter((r) => reportOnly(r) && (r.status === "over" || r.status === "not measured"))) {
    console.log(`REPORT ONLY (release gate): ${r.name} is ${r.status}; checked with --release on a quiet machine`);
  }
  const failed = rows.filter(
    (r) => !reportOnly(r) && (r.status === "over" || (requireAll && r.status === "not measured")),
  );
  if (failed.length > 0) {
    console.error(`perf budget check FAILED: ${failed.map((r) => r.name).join(", ")}`);
    process.exit(1);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main();
}
