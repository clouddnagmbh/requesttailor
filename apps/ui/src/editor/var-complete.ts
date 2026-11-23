// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Variable completion, the parts without CodeMirror: entries from the scope the backend lists
// (`VariablesApi.scope`), fuzzy ranking (prefix matches first), the texts of a row, finding an
// open `{{` before the cursor, and the edit that completes it without doubling `}}`.
// Never sees a secret value: secrets come with `value: null`.

import type { ScopeVar, SecretProvider, VariableScope, VarSource } from "../commands/types";

export interface CompletionEntry {
  name: string;
  /** `undefined`: used somewhere in the collection, but no layer here defines it. */
  kind: "variable" | "undefined";
  /** As defined; `null` for secrets, value-less and undefined names. */
  value: string | null;
  secret: boolean;
  external: SecretProvider | null;
  source: VarSource | null;
  /** The source of the definition that wins over this one, if any. */
  overriddenBy: VarSource | null;
}

/** Layer order, lowest first (as `rt_core::vars::Scope`). */
const LAYER_ORDER: VarSource["layer"][] = ["global", "collection", "environment", "folder", "request", "runtime"];

/** Rows of the list: every definition of every layer (overridden ones marked), then the
 *  undefined names. */
export function entriesFrom(scope: VariableScope): CompletionEntry[] {
  const winner = new Map<string, ScopeVar>();
  for (const v of scope.variables) if (!v.overridden) winner.set(v.name, v);
  const entries: CompletionEntry[] = scope.variables.map((v) => ({
    name: v.name,
    kind: "variable",
    value: v.secret ? null : v.value,
    secret: v.secret,
    external: v.external,
    source: v.source,
    overriddenBy: v.overridden ? (winner.get(v.name)?.source ?? null) : null,
  }));
  for (const name of scope.undefined) {
    if (winner.has(name)) continue;
    entries.push({
      name,
      kind: "undefined",
      value: null,
      secret: false,
      external: null,
      source: null,
      overriddenBy: null,
    });
  }
  return entries;
}

export interface NameMatch {
  /** 0: prefix, 1: substring, 2: letters in order. */
  rank: 0 | 1 | 2;
  /** Lower is better within a rank. */
  score: number;
  /** Matched character ranges of the name, flat `[from, to, from, to, ...]`. */
  ranges: number[];
}

const boundary = (name: string, i: number) =>
  i === 0 || /[^A-Za-z0-9]/.test(name[i - 1]) || (/[A-Z]/.test(name[i]) && /[a-z0-9]/.test(name[i - 1]));

/** Case-insensitive match of `query` in `name`, or `null`. */
export function matchName(query: string, name: string): NameMatch | null {
  if (query === "") return { rank: 0, score: 0, ranges: [] };
  const q = query.toLowerCase();
  const n = name.toLowerCase();
  if (n.startsWith(q)) return { rank: 0, score: 0, ranges: [0, q.length] };
  const at = n.indexOf(q);
  if (at > 0) {
    // A match at a word start (`user_id`, `userId`) beats one inside a word.
    let best = at;
    for (let i = at; i >= 0; i = n.indexOf(q, i + 1)) {
      if (boundary(name, i)) {
        best = i;
        break;
      }
    }
    return { rank: 1, score: (boundary(name, best) ? 0 : 1000) + best, ranges: [best, best + q.length] };
  }
  // Letters in order, preferring word starts.
  const ranges: number[] = [];
  let pos = 0;
  let first = -1;
  let last = -1;
  let starts = 0;
  for (const ch of q) {
    let i = -1;
    for (let k = pos; k < n.length; k++) {
      if (n[k] !== ch) continue;
      if (i < 0) i = k;
      if (boundary(name, k)) {
        i = k;
        break;
      }
    }
    if (i < 0) return null;
    if (boundary(name, i)) starts++;
    if (first < 0) first = i;
    last = i;
    if (ranges.length > 0 && ranges[ranges.length - 1] === i) ranges[ranges.length - 1] = i + 1;
    else ranges.push(i, i + 1);
    pos = i + 1;
  }
  return { rank: 2, score: last - first - starts * 2 + first, ranges };
}

export interface RankedEntry {
  entry: CompletionEntry;
  match: NameMatch;
}

const layerIndex = (s: VarSource | null) => (s ? LAYER_ORDER.indexOf(s.layer) : -1);

/** The entries matching `query`: prefix matches first, then word starts and substrings, then
 *  letters in order; by name within a rank; for one name the winning definition first, then
 *  the overridden ones from higher to lower layer; undefined names last. */
export function rankEntries(entries: readonly CompletionEntry[], query: string): RankedEntry[] {
  const out: RankedEntry[] = [];
  for (const entry of entries) {
    const match = matchName(query, entry.name);
    if (match) out.push({ entry, match });
  }
  out.sort(
    (a, b) =>
      Number(a.entry.kind === "undefined") - Number(b.entry.kind === "undefined") ||
      a.match.rank - b.match.rank ||
      a.match.score - b.match.score ||
      a.entry.name.localeCompare(b.entry.name, undefined, { sensitivity: "base" }) ||
      (a.entry.name < b.entry.name ? -1 : a.entry.name > b.entry.name ? 1 : 0) ||
      Number(a.entry.overriddenBy !== null) - Number(b.entry.overriddenBy !== null) ||
      layerIndex(b.entry.source) - layerIndex(a.entry.source),
  );
  return out;
}

export const PROVIDERS: Record<SecretProvider, string> = {
  "hashicorp-vault-cloud": "HCP Vault",
  "hashicorp-vault-server": "Vault",
  "aws-secrets-manager": "AWS Secrets Manager",
  "azure-key-vault": "Azure Key Vault",
  "gcp-secrets-manager": "GCP Secret Manager",
  "one-password": "1Password",
};

/** Characters of a value shown in a row. */
export const VALUE_CHARS = 48;
export const SECRET_MASK = "••••";

/** What a row shows as the value. Never a secret value. */
export function valueText(entry: CompletionEntry): string {
  if (entry.kind === "undefined") return "not defined";
  if (entry.external) return `from ${PROVIDERS[entry.external] ?? entry.external}`;
  if (entry.secret) return SECRET_MASK;
  if (entry.value === null) return "no value";
  if (entry.value === "") return "empty";
  const flat = entry.value.replace(/\s+/g, " ");
  return flat.length > VALUE_CHARS ? `${flat.slice(0, VALUE_CHARS - 1)}…` : flat;
}

/** The badge of a layer: a short label and the name of the environment or folder. */
export function layerBadge(source: VarSource): { label: string; detail: string | null; title: string } {
  switch (source.layer) {
    case "environment":
      return { label: "Env", detail: source.name, title: `Environment ${source.name}` };
    case "folder": {
      const last = source.path.split("/").pop() ?? source.path;
      return { label: "Folder", detail: last, title: `Folder ${source.path}` };
    }
    case "collection":
      return { label: "Collection", detail: null, title: "Collection" };
    case "request":
      return { label: "Request", detail: null, title: "Request" };
    case "runtime":
      return { label: "Runtime", detail: null, title: "Runtime (scripts and this session)" };
    case "global":
      return { label: "Global", detail: null, title: "Global" };
  }
}

/** "overridden by Env Local" for the marker's tooltip. */
export function overrideText(entry: CompletionEntry): string | null {
  if (!entry.overriddenBy) return null;
  return `Overridden by ${layerBadge(entry.overriddenBy).title}`;
}

/**
 * An open placeholder before column `col` of `line`: the last `{{` with no `}` and no
 * whitespace between it and the cursor. `start` is the column after `{{`, `query` what was
 * typed since.
 */
export function openPlaceholder(line: string, col: number): { start: number; query: string } | null {
  const before = line.slice(0, col);
  const at = before.lastIndexOf("{{");
  if (at < 0) return null;
  // `{{{` : the placeholder starts at the last two braces.
  const start = at + 2;
  const query = before.slice(start);
  if (/[{}\s]/.test(query)) return null;
  return { start, query };
}

/** A valid name for a new variable: what `{{name}}` resolves (no braces, no surrounding
 *  whitespace). */
export function validName(name: string): boolean {
  return name !== "" && name.trim() === name && !/[{}]/.test(name) && !/[\u0000-\u001f]/.test(name);
}

/**
 * The edit that completes the placeholder opened at column `start` with `name`, the cursor
 * at `col`: replaces what was typed; inside an existing `{{…}}` also the rest of the name, and
 * its `}}` is kept, never doubled. Offsets are columns of `line`.
 */
export function completionEdit(
  line: string,
  start: number,
  col: number,
  name: string,
): { from: number; to: number; insert: string; cursor: number } {
  const after = line.slice(col);
  // Editing an existing placeholder (`{{ba|seUrl}}`): its rest and `}}` are replaced. Else
  // nothing after the cursor is touched (a word, a quote or a brace of the text around).
  const word = /^[\w.$-]*/.exec(after)![0];
  const to = after.startsWith("}}", word.length) ? col + word.length + 2 : col;
  return { from: start, to, insert: `${name}}}`, cursor: start + name.length + 2 };
}
