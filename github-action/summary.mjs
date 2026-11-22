// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Outputs and the job summary of the GitHub Action from the JSON report of `rt run`
// ({"version": 1, "summary": ..., "steps": [...]}, crates/rt-cli/README.md). No
// dependencies.
//
// Usage: node summary.mjs <report.json> <exit-code> <output-file> <summary-file>

import { appendFileSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/** Rows in the table at most; the job summary is limited to 1 MiB. */
export const MAX_ROWS = 300;

/** Text for a Markdown table cell: one line, no pipes, no HTML, no Markdown markup. */
export function cell(text) {
  return String(text ?? "")
    .replace(/[\u0000-\u001f\u007f]+/g, " ")
    .replace(/\\/g, "\\\\")
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/[|`*_[\]]/g, (m) => `\\${m}`)
    .trim();
}

function ratio(list) {
  const all = list ?? [];
  if (all.length === 0) return "";
  return `${all.filter((x) => x.passed).length}/${all.length}`;
}

function errorText(error) {
  if (!error) return "";
  const m = error.message;
  if (typeof m === "string") return `${error.kind}: ${m}`;
  if (m && typeof m === "object") return `${error.kind}: ${m.kind}${m.message ? `: ${m.message}` : ""}`;
  return String(error.kind);
}

/** The step outputs (name -> value). */
export function outputs(report) {
  const s = report.summary ?? {};
  return {
    passed: s.passed ?? 0,
    failed: s.failed ?? 0,
    skipped: s.skipped ?? 0,
    "tests-passed": s.tests_passed ?? 0,
    "tests-failed": s.tests_failed ?? 0,
    "assertions-passed": s.assertions_passed ?? 0,
    "assertions-failed": s.assertions_failed ?? 0,
  };
}

/** The job summary as Markdown. */
export function markdown(report, exitCode) {
  const s = report.summary ?? {};
  const steps = report.steps ?? [];
  const iterations = new Set(steps.map((st) => st.iteration)).size;
  const ok = Number(exitCode) === 0;
  const lines = [];
  lines.push(`### ${ok ? "✅" : "❌"} rt run: ${s.passed ?? 0} of ${s.requests ?? steps.length} requests passed`);
  lines.push("");
  const facts = [
    `${s.failed ?? 0} failed`,
    `${s.skipped ?? 0} skipped`,
    `tests ${s.tests_passed ?? 0} passed, ${s.tests_failed ?? 0} failed`,
    `assertions ${s.assertions_passed ?? 0} passed, ${s.assertions_failed ?? 0} failed`,
    `${((s.duration_ms ?? 0) / 1000).toFixed(2)} s`,
  ];
  if (s.stopped) facts.push("stopped early");
  if (!ok && Number(exitCode) !== 4) facts.push(`exit code ${exitCode}`);
  lines.push(facts.join(" · "));
  lines.push("");
  if (steps.length > 0) {
    const head = ["", ...(iterations > 1 ? ["#"] : []), "Request", "Status", "Time", "Tests", "Assertions"];
    lines.push(`| ${head.join(" | ")} |`);
    lines.push(`|${head.map(() => " --- ").join("|")}|`);
    for (const st of steps.slice(0, MAX_ROWS)) {
      const r = st.report ?? {};
      const mark = st.skipped ? "⏭️" : st.passed ? "✅" : "❌";
      const method = r.request?.method ?? "";
      const name = `${method ? `${method} ` : ""}${st.request_name || st.request_path}`;
      const status = r.response ? String(r.response.status) : r.error ? errorText(r.error) : st.skipped ? "skipped" : "";
      const time = r.response ? `${Math.round(r.response.total)} ms` : "";
      const row = [
        mark,
        ...(iterations > 1 ? [String((st.iteration ?? 0) + 1)] : []),
        `${cell(name)} <sub>${cell(st.request_path)}</sub>`,
        cell(status),
        time,
        ratio(r.tests),
        ratio(r.assertions),
      ];
      lines.push(`| ${row.join(" | ")} |`);
    }
    if (steps.length > MAX_ROWS) lines.push("", `… and ${steps.length - MAX_ROWS} more requests (see the JSON report).`);
  }
  const failures = [];
  for (const st of steps) {
    const r = st.report ?? {};
    const where = `${cell(st.request_name || st.request_path)}${iterations > 1 ? ` (iteration ${(st.iteration ?? 0) + 1})` : ""}`;
    for (const t of r.tests ?? []) if (!t.passed) failures.push(`- **${where}**: ${cell(t.name)}: ${cell(t.error ?? "failed")}`);
    for (const a of r.assertions ?? []) {
      if (!a.passed) {
        const name = [a.expression, a.operator, a.expected].filter(Boolean).join(" ");
        const why = a.error ?? (a.actual != null ? `got ${a.actual}` : "did not evaluate");
        failures.push(`- **${where}**: ${cell(name)}: ${cell(why)}`);
      }
    }
  }
  if (failures.length > 0) {
    lines.push("", "<details open><summary>Failed tests and assertions</summary>", "");
    lines.push(...failures.slice(0, MAX_ROWS));
    if (failures.length > MAX_ROWS) lines.push(`- … and ${failures.length - MAX_ROWS} more`);
    lines.push("", "</details>");
  }
  lines.push("");
  return lines.join("\n");
}

export function main(argv) {
  const [reportPath, exitCode, outputFile, summaryFile] = argv;
  const report = JSON.parse(readFileSync(reportPath, "utf8"));
  if (report.version !== 1) throw new Error(`unknown report version ${report.version}`);
  const out = Object.entries(outputs(report)).map(([k, v]) => `${k}=${v}\n`).join("");
  appendFileSync(outputFile, out);
  if (summaryFile && summaryFile !== "/dev/null") appendFileSync(summaryFile, markdown(report, exitCode));
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  main(process.argv.slice(2));
}
