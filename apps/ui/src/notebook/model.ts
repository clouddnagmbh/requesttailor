// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the notebook view (wave 19, strand W): what a run's results become on screen.
// Results live in memory only; nothing here saves an output.

import type { BlockResult, Notebook, NotebookBlock, NotebookShow, SendReport } from "../commands/types";

export const MASK = "***";
export const SUFFIX = ".notebook.md";

export type RequestBlock = Extract<NotebookBlock, { kind: "request" }>;

export const isRequest = (b: NotebookBlock): b is RequestBlock => b.kind === "request";

/** The request file or the URL of an embedded request: what a block is, in one short line. */
export function blockLabel(block: RequestBlock): string {
  if (block.ref) return block.ref;
  const spec = block.embedded;
  return spec ? spec.url : block.id;
}

/** The variables the results so far carry, newest value wins; a masked value is not carried. */
export function carriedVars(results: Iterable<BlockResult>): [string, string][] {
  const out = new Map<string, string>();
  for (const r of results) for (const [n, v] of r.set ?? []) if (v !== MASK) out.set(n, v);
  return [...out];
}

/** The status line of a report: "200 12 ms 34 B", the guard's refusal or the error. */
export function statusLine(report: SendReport | undefined): string {
  if (!report) return "";
  if (report.response) {
    const r = report.response;
    return `${r.status}  ${Math.round(r.total)} ms  ${r.size} B`;
  }
  const e = report.error;
  if (!e) return "";
  if (e.kind === "blocked") return `refused by the send guard (${e.message.rule}): ${e.message.reason}`;
  return `failed: ${typeof e.message === "string" ? e.message : JSON.stringify(e.message)}`;
}

/** `true` if the block failed to send, or a test or assertion of it failed. */
export function failed(report: SendReport | undefined): boolean {
  if (!report) return false;
  return (
    report.error != null ||
    (report.tests ?? []).some((t) => !t.passed) ||
    (report.assertions ?? []).some((a) => !a.passed)
  );
}

/** The response body as shown: pretty JSON if it is JSON, else the text; cut at `limit` characters. */
export function bodyText(report: SendReport | undefined, limit = 6000): { text: string; cut: boolean } {
  const body = report?.response?.body;
  if (!body || body.encoding !== "utf8") return { text: body ? "(binary)" : "", cut: false };
  let text = body.data;
  try {
    text = JSON.stringify(JSON.parse(text), null, 2);
  } catch {
    /* not JSON: as received */
  }
  return text.length > limit ? { text: text.slice(0, limit), cut: true } : { text, cut: false };
}

export interface ChartPoint {
  x: number;
  y: number;
  value: number;
}

/** Points of a polyline for `values` in a `width` x `height` box (non-numbers are left out). */
export function chartPoints(values: unknown[], width: number, height: number, pad = 6): ChartPoint[] {
  const nums = values.map((v) => (typeof v === "number" ? v : typeof v === "string" ? Number(v) : NaN)).filter(Number.isFinite);
  if (nums.length === 0) return [];
  const min = Math.min(...nums);
  const max = Math.max(...nums);
  const span = max - min || 1;
  const step = nums.length > 1 ? (width - 2 * pad) / (nums.length - 1) : 0;
  return nums.map((value, i) => ({
    x: nums.length > 1 ? pad + i * step : width / 2,
    y: height - pad - ((value - min) / span) * (height - 2 * pad),
    value,
  }));
}

export const showLabel = (s: NotebookShow): string =>
  s.kind === "response" ? "response" : `${s.kind} ${s.path}`;

/** A starter notebook for a new file. */
export function starter(title: string): Notebook {
  return {
    title,
    blocks: [
      {
        kind: "text",
        markdown: `# ${title}\n\nWrite what you want to show here. Request blocks run one after another and pass variables on.\n\n`,
      },
    ],
  };
}

/** `<name>.notebook.md` that no listed file has, from a free-text name. */
export function newPath(name: string, taken: string[]): string {
  const base = name.trim().toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "") || "notebook";
  let path = `${base}${SUFFIX}`;
  for (let n = 2; taken.includes(path); n++) path = `${base}-${n}${SUFFIX}`;
  return path;
}
