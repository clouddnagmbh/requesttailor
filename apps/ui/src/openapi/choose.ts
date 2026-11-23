// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The "Choose" step of the OpenAPI import wizard as data: the operations as a tree (by tag,
// by path or flat), selection with search and select all/none, the servers that become
// environments (with editable names) and the one that is the default, and the options the
// core converts with. Pure, unit-tested in choose.test.ts.

import type {
  OpenApiExamples,
  OpenApiGrouping,
  OpenApiNaming,
  OpenApiOptions,
  SpecOperation,
  SpecOverview,
} from "../commands/types";

export interface OperationGroup {
  /** Stable key: the tag, the first path segment, or `""` (flat / untagged). */
  key: string;
  /** Shown name; `""` for flat. */
  label: string;
  description: string | null;
  operations: SpecOperation[];
}

/** The folder a request lands in for `grouping` (as the core does: first tag; first path
 *  segment; none). */
export function groupOf(op: SpecOperation, grouping: OpenApiGrouping): string {
  if (grouping === "tag") return op.tags[0] ?? "";
  if (grouping === "path") return op.path.split("/").find((s) => s !== "" && !s.startsWith("{")) ?? "";
  return "";
}

/** Operations grouped as the import will file them; groups in the spec's tag order (path:
 *  first appearance), operations without a group last. */
export function groupOperations(spec: SpecOverview, grouping: OpenApiGrouping): OperationGroup[] {
  const groups = new Map<string, OperationGroup>();
  if (grouping === "tag") {
    for (const t of spec.tags) groups.set(t.name, { key: t.name, label: t.name, description: t.description, operations: [] });
  }
  const loose: SpecOperation[] = [];
  for (const op of spec.operations) {
    const key = groupOf(op, grouping);
    if (key === "") {
      loose.push(op);
      continue;
    }
    let g = groups.get(key);
    if (!g) {
      g = { key, label: key, description: null, operations: [] };
      groups.set(key, g);
    }
    g.operations.push(op);
  }
  const out = [...groups.values()].filter((g) => g.operations.length > 0);
  if (loose.length) out.push({ key: "", label: grouping === "flat" ? "" : "Other", description: null, operations: loose });
  return out;
}

/** The name the request gets, as the core names it. */
export function requestName(op: SpecOperation, naming: OpenApiNaming): string {
  const summary = op.summary?.trim() || null;
  const id = op.operation_id?.trim() || null;
  const first = naming === "operation-id" ? (id ?? summary) : (summary ?? id);
  return (first ?? `${op.method} ${op.path}`).split("\n")[0]!.slice(0, 120);
}

/** Case-insensitive search over method, path, name, operationId and tags. */
export function matches(op: SpecOperation, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (q === "") return true;
  const hay = [op.method, op.path, op.summary ?? "", op.operation_id ?? "", ...op.tags].join(" ").toLowerCase();
  return q.split(/\s+/).every((word) => hay.includes(word));
}

export function filterGroups(groups: OperationGroup[], query: string): OperationGroup[] {
  return groups
    .map((g) => ({ ...g, operations: g.operations.filter((op) => matches(op, query) || (query.trim() !== "" && g.label.toLowerCase().includes(query.trim().toLowerCase()))) }))
    .filter((g) => g.operations.length > 0);
}

export type Tri = "all" | "some" | "none";

export function groupState(group: OperationGroup, selected: ReadonlySet<string>): Tri {
  const n = group.operations.filter((op) => selected.has(op.key)).length;
  return n === 0 ? "none" : n === group.operations.length ? "all" : "some";
}

/** Selects (or clears) every operation in `ops`. */
export function setMany(selected: ReadonlySet<string>, ops: readonly SpecOperation[], on: boolean): Set<string> {
  const out = new Set(selected);
  for (const op of ops) {
    if (on) out.add(op.key);
    else out.delete(op.key);
  }
  return out;
}

export interface ServerRow {
  index: number;
  url: string;
  description: string | null;
  /** Becomes an environment. */
  include: boolean;
  /** Its name, editable. */
  name: string;
}

/** Suggested names: "Production", "Staging" … where the URL or description says so, else the
 *  core's suggestion (description's first line, else the host). */
export function suggestName(url: string, description: string | null, fallback: string): string {
  const text = `${description ?? ""} ${url}`.toLowerCase();
  const known: [RegExp, string][] = [
    [/\b(prod|production|live)\b/, "Production"],
    [/\b(stag|staging|stage)\b/, "Staging"],
    [/\b(sandbox)\b/, "Sandbox"],
    [/\b(test|testing|qa)\b/, "Test"],
    [/\b(dev|development)\b/, "Development"],
    [/\b(localhost|127\.0\.0\.1|local)\b/, "Local"],
  ];
  if (!description?.trim()) for (const [re, name] of known) if (re.test(text)) return name;
  return safeName(fallback);
}

/** An environment name without the characters file names cannot have ("host/path" → "host"). */
export function safeName(name: string): string {
  const clean = name.split("/")[0]!.replace(/[\\:*?"<>|]+/g, "-").replace(/^-+|-+$/g, "").trim();
  return clean || "Server";
}

export function serverRows(spec: SpecOverview): ServerRow[] {
  const rows = spec.servers.map((s, index) => ({
    index,
    url: s.url,
    description: s.description,
    include: true,
    name: suggestName(s.url, s.description, s.suggested_name),
  }));
  // Unique names, as the core makes them.
  const seen = new Map<string, number>();
  for (const r of rows) {
    const n = (seen.get(r.name) ?? 0) + 1;
    seen.set(r.name, n);
    if (n > 1) r.name = `${r.name} (${n})`;
  }
  return rows;
}

/** Problems of the server rows, by index: empty or duplicate names. */
export function serverErrors(rows: readonly ServerRow[]): Record<number, string> {
  const out: Record<number, string> = {};
  const names = new Map<string, number>();
  for (const r of rows) {
    if (!r.include) continue;
    const name = r.name.trim();
    if (name === "") out[r.index] = "Give the environment a name.";
    else if (/[/\\:*?"<>|]/.test(name)) out[r.index] = "Names cannot contain / \\ : * ? \" < > |";
    else if (names.has(name.toLowerCase())) out[r.index] = "Another environment has this name.";
    names.set(name.toLowerCase(), r.index);
  }
  return out;
}

export interface Choices {
  selected: ReadonlySet<string>;
  grouping: OpenApiGrouping;
  naming: OpenApiNaming;
  examples: OpenApiExamples;
  servers: readonly ServerRow[];
  defaultServer: number | null;
  collectionName: string;
  keepSpec: boolean;
}

export function initialChoices(spec: SpecOverview): Choices {
  return {
    selected: new Set(spec.operations.map((o) => o.key)),
    grouping: spec.tags.length > 0 ? "tag" : "path",
    naming: "summary",
    examples: "spec",
    servers: serverRows(spec),
    defaultServer: spec.servers.length > 0 ? 0 : null,
    collectionName: spec.title,
    keepSpec: true,
  };
}

/** The options the core converts with. */
export function toOptions(spec: SpecOverview, c: Choices): OpenApiOptions {
  const all = spec.operations.every((o) => c.selected.has(o.key));
  return {
    operations: all ? null : spec.operations.filter((o) => c.selected.has(o.key)).map((o) => o.key),
    grouping: c.grouping,
    naming: c.naming,
    examples: c.examples,
    servers: c.servers.map((r) => ({ index: r.index, environment: r.include ? r.name.trim() : null })),
    default_server: c.defaultServer,
    collection_name: c.collectionName.trim() || null,
    keep_spec: c.keepSpec,
  };
}

/** Counts for the summary line of the "Choose" step. */
export function summaryOf(spec: SpecOverview, c: Choices): { requests: number; folders: number; environments: number } {
  const ops = spec.operations.filter((o) => c.selected.has(o.key));
  const folders = new Set(ops.map((o) => groupOf(o, c.grouping)).filter((k) => k !== ""));
  return { requests: ops.length, folders: folders.size, environments: c.servers.filter((r) => r.include).length };
}
