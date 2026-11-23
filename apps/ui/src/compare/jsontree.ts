// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// JSON tree diff of two response bodies (wave 14, strand H): both values as one tree, each
// node marked same, added, removed or changed, containers with the number of changed values
// below them. `treeLines` flattens it into the visible lines of the side-by-side view, with
// unchanged containers folded by default so the eye lands on what differs.

export type NodeKind = "same" | "added" | "removed" | "changed";
export type Container = "object" | "array";

export interface JsonNode {
  /** Property name or array index; `null` for the root. */
  key: string | null;
  /** JSON Pointer (`""` for the root), as `BodyChange.pointer`. */
  pointer: string;
  kind: NodeKind;
  /** Set when both sides are the same kind of container (or only one side exists and is one). */
  container: Container | null;
  old?: unknown;
  new?: unknown;
  children: JsonNode[];
  /** Changed, added and removed leaves at or below this node. */
  changes: number;
}

const containerOf = (v: unknown): Container | null =>
  Array.isArray(v) ? "array" : typeof v === "object" && v !== null ? "object" : null;

const token = (key: string) => key.replace(/~/g, "~0").replace(/\//g, "~1");

function entries(v: unknown, c: Container): [string, unknown][] {
  return c === "array" ? (v as unknown[]).map((x, i) => [String(i), x]) : Object.entries(v as Record<string, unknown>);
}

function sameJson(a: unknown, b: unknown): boolean {
  if (a === b) return true;
  const [ca, cb] = [containerOf(a), containerOf(b)];
  if (ca === null || ca !== cb) return false;
  const [ea, eb] = [entries(a, ca), entries(b, cb!)];
  if (ea.length !== eb.length) return false;
  if (ca === "array") return ea.every(([, v], i) => sameJson(v, eb[i]![1]));
  const mb = new Map(eb);
  return ea.every(([k, v]) => mb.has(k) && sameJson(v, mb.get(k)));
}

/** One side only (an added or removed value), with its children for unfolding. */
function oneSide(key: string | null, pointer: string, value: unknown, kind: "added" | "removed"): JsonNode {
  const container = containerOf(value);
  const children = container ? entries(value, container).map(([k, v]) => oneSide(k, `${pointer}/${token(k)}`, v, kind)) : [];
  return {
    key,
    pointer,
    kind,
    container,
    ...(kind === "removed" ? { old: value } : { new: value }),
    children,
    changes: container ? Math.max(1, children.reduce((n, c) => n + c.changes, 0)) : 1,
  };
}

/** The tree of `old` → `now`. `hasOld`/`hasNew` false: that side has no body at all. */
export function diffTree(old: unknown, now: unknown, hasOld = true, hasNew = true, key: string | null = null, pointer = ""): JsonNode {
  if (!hasOld) return oneSide(key, pointer, now, "added");
  if (!hasNew) return oneSide(key, pointer, old, "removed");
  const [co, cn] = [containerOf(old), containerOf(now)];
  if (co !== null && co === cn) {
    const a = entries(old, co);
    const b = new Map(entries(now, cn));
    const keys = [...a.map(([k]) => k), ...[...b.keys()].filter((k) => !a.some(([x]) => x === k))];
    const ma = new Map(a);
    const children = keys.map((k) => diffTree(ma.get(k), b.get(k), ma.has(k), b.has(k), k, `${pointer}/${token(k)}`));
    const changes = children.reduce((n, c) => n + c.changes, 0);
    return { key, pointer, kind: changes > 0 ? "changed" : "same", container: co, old, new: now, children, changes };
  }
  const same = sameJson(old, now);
  return { key, pointer, kind: same ? "same" : "changed", container: null, old, new: now, children: [], changes: same ? 0 : 1 };
}

export interface TreeLine {
  pointer: string;
  depth: number;
  key: string | null;
  kind: NodeKind;
  container: Container | null;
  /** Containers: unfolded. */
  open: boolean;
  /** The text of each side (`null`: nothing on that side). Folded containers show a summary,
   *  open ones their opening bracket. */
  old: string | null;
  new: string | null;
  changes: number;
  /** The closing bracket of an open container. */
  closing?: boolean;
}

/** One line of a value: JSON, cut to `max` characters. */
export function leafText(v: unknown, max = 160): string {
  const text = v === undefined ? "" : JSON.stringify(v) ?? String(v);
  return text.length > max ? `${text.slice(0, max - 1)}…` : text;
}

function folded(v: unknown, c: Container): string {
  const n = entries(v, c).length;
  return c === "array" ? `[ ${n} ${n === 1 ? "item" : "items"} ]` : `{ ${n} ${n === 1 ? "key" : "keys"} }`;
}

const hasSide = (n: JsonNode, side: "old" | "new") => (side === "old" ? n.kind !== "added" : n.kind !== "removed");

/** The visible lines of `root`, unfolding the containers `isOpen` says are open. */
export function treeLines(root: JsonNode, isOpen: (pointer: string) => boolean, limit = 20_000): TreeLine[] {
  const out: TreeLine[] = [];
  const walk = (n: JsonNode, depth: number) => {
    if (out.length >= limit) return;
    const open = n.container !== null && isOpen(n.pointer);
    const text = (side: "old" | "new"): string | null => {
      if (!hasSide(n, side)) return null;
      const v = side === "old" ? n.old : n.new;
      if (n.container) return open ? (n.container === "array" ? "[" : "{") : folded(v, n.container);
      return leafText(v);
    };
    out.push({ pointer: n.pointer, depth, key: n.key, kind: n.kind, container: n.container, open, old: text("old"), new: text("new"), changes: n.changes });
    if (!open) return;
    for (const c of n.children) walk(c, depth + 1);
    if (out.length >= limit) return;
    const close = n.container === "array" ? "]" : "}";
    out.push({
      pointer: `${n.pointer}\u0000end`,
      depth,
      key: null,
      kind: n.kind,
      container: n.container,
      open: true,
      old: hasSide(n, "old") ? close : null,
      new: hasSide(n, "new") ? close : null,
      changes: 0,
      closing: true,
    });
  };
  walk(root, 0);
  return out;
}

/** Containers open at first: the root and every container with a change below it, unless it
 *  only exists on one side (an added object stays folded; its summary says enough). */
export function initiallyOpen(root: JsonNode, max = 400): Set<string> {
  const open = new Set<string>();
  const walk = (n: JsonNode) => {
    if (open.size >= max || n.container === null) return;
    if (n.pointer === "" || n.kind === "changed") {
      open.add(n.pointer);
      for (const c of n.children) walk(c);
    }
  };
  walk(root);
  return open;
}

/** Parses a body preview as JSON; `undefined` when it is not JSON (or cut off). */
export function parseJson(text: string | undefined | null): { value: unknown } | undefined {
  if (text === undefined || text === null || text.trim() === "") return undefined;
  try {
    return { value: JSON.parse(text) as unknown };
  } catch {
    return undefined;
  }
}
