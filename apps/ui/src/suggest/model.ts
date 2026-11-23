// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the suggestion view shows (wave 16, strand C): a suggested change as blocks. A block is
// one field (a request's URL, one header value) or one text (a script, a file); the view marks
// the changed words of a one-line value and the changed lines of a text.

import type { RequestSpec, Script } from "../commands/types";
import { lineOps, type LineOp } from "../diff/textdiff";

export interface PreviewBlock {
  /** Plain-language name of the field or text, e.g. `Header › X-Request-Id`. */
  label: string;
  /** `null`: did not exist before. */
  old: string | null;
  /** `null`: removed. */
  next: string | null;
  /** Shown as code (line diff) even when it is one line. */
  code: boolean;
}

export interface DiffStats {
  added: number;
  removed: number;
}

export const isMulti = (b: PreviewBlock): boolean => b.code || (b.old ?? "").includes("\n") || (b.next ?? "").includes("\n");

/** Changed lines of a text block (a one-line field counts as one line each side). */
export function blockStats(b: PreviewBlock): DiffStats {
  if (b.old === b.next) return { added: 0, removed: 0 };
  if (!isMulti(b)) return { added: b.next === null ? 0 : 1, removed: b.old === null ? 0 : 1 };
  const ops = lineOps(b.old, b.next);
  return { added: ops.filter((o) => o.kind === "added").length, removed: ops.filter((o) => o.kind === "removed").length };
}

export function totalStats(blocks: readonly PreviewBlock[]): DiffStats {
  return blocks.reduce(
    (sum, b) => {
      const s = blockStats(b);
      return { added: sum.added + s.added, removed: sum.removed + s.removed };
    },
    { added: 0, removed: 0 },
  );
}

/** A line of the unified diff with both line numbers (`null` on the side it is not on). */
export interface DiffRow {
  kind: LineOp["kind"] | "gap";
  text: string;
  oldLine: number | null;
  newLine: number | null;
  /** `gap`: how many unchanged lines are hidden. */
  hidden?: number;
}

/** The unified rows of `old` → `next` with `context` unchanged lines around changes; longer
 *  stretches of unchanged lines become one `gap` row. */
export function diffRows(old: string | null, next: string | null, context = 3): DiffRow[] {
  const ops = lineOps(old, next);
  const rows: DiffRow[] = [];
  let a = 0;
  let b = 0;
  for (const op of ops) {
    if (op.kind === "same") rows.push({ kind: "same", text: op.text, oldLine: ++a, newLine: ++b });
    else if (op.kind === "removed") rows.push({ kind: "removed", text: op.text, oldLine: ++a, newLine: null });
    else rows.push({ kind: "added", text: op.text, oldLine: null, newLine: ++b });
  }
  const keep = rows.map((r) => r.kind !== "same");
  const near = keep.map((_, i) => {
    for (let d = -context; d <= context; d++) if (keep[i + d]) return true;
    return false;
  });
  const out: DiffRow[] = [];
  let hidden = 0;
  rows.forEach((r, i) => {
    if (near[i]) {
      if (hidden > 0) out.push({ kind: "gap", text: "", oldLine: null, newLine: null, hidden });
      hidden = 0;
      out.push(r);
    } else hidden++;
  });
  if (hidden > 0 && out.length > 0) out.push({ kind: "gap", text: "", oldLine: null, newLine: null, hidden });
  return out;
}

// ---- requests ----

const WORDS: Record<string, string> = {
  url: "URL",
  seq: "Order",
  vars: "Variables",
  auth: "Authentication",
  params: "Parameters",
  headers: "Header",
  scripts: "Script",
  assertions: "Assertion",
  actions: "Action",
  settings: "Setting",
  method: "Method",
  name: "Name",
  body: "Body",
};

/** `name: value` rows (headers, parameters, variables) read as one line, not as three fields. */
function isPair(item: unknown): item is { name: string; value?: string | null; enabled?: boolean; secret?: boolean } {
  const o = item as Record<string, unknown> | null;
  return !!o && typeof o === "object" && typeof o.name === "string" && "value" in o && (o.value === null || typeof o.value === "string");
}

function leaves(value: unknown, path: string[], out: Map<string, { path: string[]; text: string }>) {
  if (value === null || value === undefined) return;
  if (Array.isArray(value)) {
    value.forEach((item, i) => {
      if (isPair(item)) {
        const shown = item.secret ? "(secret)" : (item.value ?? "");
        const text = item.enabled === false ? `${shown}  (off)` : shown;
        const at = [...path, item.name];
        out.set(at.join("\u0000"), { path: at, text });
        return;
      }
      const o = item as Record<string, unknown> | null;
      // Name items by their name or phase, so a reordered list is not a pile of changes.
      const key = o && typeof o === "object" ? String(o.name ?? o.phase ?? o.expression ?? i) : String(i);
      leaves(item, [...path, key], out);
    });
  } else if (typeof value === "object") {
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) leaves(v, [...path, k], out);
  } else {
    out.set(path.join("\u0000"), { path, text: String(value) });
  }
}

function labelOf(path: string[]): string {
  const [head, ...rest] = path;
  const first = WORDS[head] ?? head;
  // `headers › X-Id › value` reads better as `Header › X-Id`; the leaf name `value` is dropped.
  const tail = rest.filter((s, i) => !(i === rest.length - 1 && rest.length > 1 && (s === "value" || s === "code")));
  return [first, ...tail].join(" › ");
}

/** The fields of `next` that differ from `old` (`null`: every field of a new request counts). */
export function requestBlocks(old: RequestSpec | null, next: RequestSpec): PreviewBlock[] {
  const a = new Map<string, { path: string[]; text: string }>();
  const b = new Map<string, { path: string[]; text: string }>();
  if (old) leaves(old, [], a);
  leaves(next, [], b);
  const keys = [...new Set([...b.keys(), ...a.keys()])];
  const blocks: PreviewBlock[] = [];
  for (const key of keys) {
    const x = a.get(key);
    const y = b.get(key);
    if (x?.text === y?.text) continue;
    const path = (y ?? x)!.path;
    // A new request lists what it has, not what it lacks; skip empty and default values.
    if (!old && (y!.text === "" || y!.text === "false" || y!.text === "none")) continue;
    blocks.push({ label: labelOf(path), old: x?.text ?? null, next: y?.text ?? null, code: path[0] === "scripts" || path[0] === "body" });
  }
  return blocks;
}

const PHASE: Record<Script["phase"], string> = {
  "before-request": "Before the request",
  "after-response": "After the response",
  tests: "Tests",
};

/** The scripts of a folder or collection, one block per phase that changes. */
export function scriptBlocks(old: readonly Script[], next: readonly Script[]): PreviewBlock[] {
  const code = (list: readonly Script[], phase: Script["phase"]) => list.find((s) => s.phase === phase)?.code ?? null;
  return (Object.keys(PHASE) as Script["phase"][])
    .map((p) => ({ label: `Script › ${PHASE[p]}`, old: code(old, p), next: code(next, p), code: true }))
    .filter((b) => b.old !== b.next);
}

/** A whole text (a file, an editor's content). */
export function textBlock(label: string, old: string | null, next: string): PreviewBlock {
  return { label, old, next, code: true };
}
