// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A model of `rt_core::snapshot` for the mock backend and the UI tests: freeze a response and
// compare a later one with it, with the same file format, header allowlist, ignore paths (`$.a[*].b`,
// `res.body…`, `res.headers…`, `res.status`) and diff paths as the Rust engine. The real rules
// live in Rust and are tested there (`crates/rt-core/tests/snapshots.rs`); nothing but the mock
// reads this file.

import type { Snapshot, SnapshotBody, SnapshotDiff } from "../commands/types";

export const SNAPSHOT_VERSION = 1;
export const HEADER_ALLOWLIST = ["content-type", "content-language", "cache-control"];
/** Bytes of a text body a snapshot keeps (`rt_core::snapshot::BODY_LIMIT`). */
export const BODY_LIMIT = 256 * 1024;
export const MASK = "***";

export interface MockResponseView {
  status: number;
  headers: [string, string][];
  /** The body as text. */
  body: string;
}

type Seg = { key: string } | { index: number } | "any";

const isKey = (s: Seg): s is { key: string } => typeof s === "object" && "key" in s;
const isIndex = (s: Seg): s is { index: number } => typeof s === "object" && "index" in s;

function mask(text: string, secrets: string[]): string {
  let out = text;
  for (const s of secrets) if (s !== "") out = out.split(s).join(MASK);
  return out;
}

function keptHeaders(headers: [string, string][], keepEtag: boolean, secrets: string[]): Record<string, string> {
  const grouped = new Map<string, string[]>();
  for (const [name, value] of headers) {
    const n = name.toLowerCase();
    if (HEADER_ALLOWLIST.includes(n) || (keepEtag && n === "etag")) grouped.set(n, [...(grouped.get(n) ?? []), value]);
  }
  return Object.fromEntries([...grouped].sort(([a], [b]) => (a < b ? -1 : 1)).map(([n, v]) => [n, mask(v.join(", "), secrets)]));
}

function digest(text: string): SnapshotBody {
  let h = 0xcbf29ce484222325n;
  for (const b of new TextEncoder().encode(text)) h = ((h ^ BigInt(b)) * 0x100000001b3n) & 0xffffffffffffffffn;
  return { $digest: h.toString(16).padStart(16, "0").repeat(4), $length: new TextEncoder().encode(text).length };
}

function bodyOf(text: string, secrets: string[]): SnapshotBody {
  const masked = mask(text, secrets);
  try {
    return JSON.parse(masked) as SnapshotBody;
  } catch {
    return masked.length > BODY_LIMIT ? digest(masked) : masked;
  }
}

export function freezeResponse(response: MockResponseView, secrets: string[], options: { keepEtag?: boolean; ignore?: string[] } = {}): Snapshot {
  return {
    version: SNAPSHOT_VERSION,
    status: response.status,
    headers: keptHeaders(response.headers, options.keepEtag ?? false, secrets),
    body: bodyOf(response.body, secrets),
    ignore: options.ignore ?? [],
  };
}

/** The text of a `.snap.json` file: fixed key order, two spaces, a final newline. */
export function snapshotText(s: Snapshot): string {
  return `${JSON.stringify({ version: s.version, status: s.status, headers: s.headers, body: s.body, ignore: s.ignore }, null, 2)}\n`;
}

export function parseSnapshot(text: string): Snapshot {
  const s = JSON.parse(text) as Snapshot;
  if (s.version !== SNAPSHOT_VERSION) throw new Error(`snapshot version ${s.version} is not supported`);
  return { ...s, ignore: s.ignore ?? [] };
}

function parseIgnore(text: string): Seg[] | null {
  const t = text.trim();
  let head: string;
  let rest: string;
  if (t.startsWith("$")) {
    head = "body";
    rest = t.slice(1);
  } else {
    const u = t.startsWith("res.") ? t.slice(4) : t;
    const end = u.search(/[.[]/);
    head = end < 0 ? u : u.slice(0, end);
    rest = end < 0 ? "" : u.slice(end);
    if (!["body", "headers", "status"].includes(head)) return null;
  }
  const path: Seg[] = [{ key: head }];
  while (rest !== "") {
    let m: RegExpMatchArray | null;
    if ((m = rest.match(/^\.([^.[]+)/))) {
      path.push({ key: m[1]!.trim() });
    } else if ((m = rest.match(/^\[\s*(\*|\d+)\s*\]/))) {
      path.push(m[1] === "*" ? "any" : { index: Number(m[1]) });
    } else if ((m = rest.match(/^\[\s*(["'])(.*?)\1\s*\]/))) {
      path.push({ key: m[2]! });
    } else {
      return null;
    }
    rest = rest.slice(m[0].length);
  }
  return path;
}

/** `null` if `path` is a path the comparison can use, else why not. */
export function checkIgnore(path: string): string | null {
  return parseIgnore(path) ? null : `\`${path}\`: a path starts with \`$\`, \`res.body\`, \`res.headers\` or \`res.status\``;
}

function render(path: Seg[]): string {
  return path
    .map((s, i) => {
      if (isKey(s)) return /^[\p{L}\p{N}_$-]+$/u.test(s.key) ? `${i > 0 ? "." : ""}${s.key}` : `[${JSON.stringify(s.key)}]`;
      return isIndex(s) ? `[${s.index}]` : "[*]";
    })
    .join("");
}

function covers(pattern: Seg[], path: Seg[]): boolean {
  return (
    pattern.length <= path.length &&
    pattern.every((p, i) => {
      const s = path[i]!;
      if (p === "any") return isIndex(s);
      if (isKey(p)) return isKey(s) && s.key === p.key;
      return isIndex(p) && isIndex(s) && s.index === p.index;
    })
  );
}

const kindOf = (v: unknown) => (v === null ? "null" : Array.isArray(v) ? "array" : typeof v);

/** Compares `response` with `snapshot`, skipping its ignored paths. Empty: equal. */
export function compareResponse(snapshot: Snapshot, response: MockResponseView, secrets: string[]): SnapshotDiff[] {
  const fresh = {
    status: response.status,
    headers: keptHeaders(response.headers, "etag" in snapshot.headers, secrets),
    body: bodyOf(response.body, secrets),
  };
  const ignore = snapshot.ignore.map(parseIgnore).filter((p): p is Seg[] => p !== null);
  const out: SnapshotDiff[] = [];
  const at = (path: Seg[], kind: SnapshotDiff["kind"], expected: unknown, actual: unknown, has: [boolean, boolean]) => {
    if (ignore.some((p) => covers(p, path))) return;
    out.push({ path: render(path), kind, expected: has[0] ? expected : null, actual: has[1] ? actual : null });
  };
  const walk = (path: Seg[], a: unknown, b: unknown) => {
    if (a !== null && b !== null && typeof a === "object" && typeof b === "object" && Array.isArray(a) === Array.isArray(b)) {
      if (Array.isArray(a)) {
        const bb = b as unknown[];
        for (let i = 0; i < Math.max(a.length, bb.length); i++) {
          const p = [...path, { index: i }];
          if (i < a.length && i < bb.length) walk(p, a[i], bb[i]);
          else if (i < a.length) at(p, "removed", a[i], null, [true, false]);
          else at(p, "added", null, bb[i], [false, true]);
        }
      } else {
        const ao = a as Record<string, unknown>;
        const bo = b as Record<string, unknown>;
        for (const k of [...new Set([...Object.keys(ao), ...Object.keys(bo)])].sort()) {
          const p = [...path, { key: k }];
          if (k in ao && k in bo) walk(p, ao[k], bo[k]);
          else if (k in ao) at(p, "removed", ao[k], null, [true, false]);
          else at(p, "added", null, bo[k], [false, true]);
        }
      }
      return;
    }
    if (JSON.stringify(a) === JSON.stringify(b)) return;
    at(path, kindOf(a) !== kindOf(b) ? "type" : "changed", a, b, [true, true]);
  };
  if (snapshot.status !== fresh.status) at([{ key: "status" }], "changed", snapshot.status, fresh.status, [true, true]);
  for (const name of [...new Set([...Object.keys(snapshot.headers), ...Object.keys(fresh.headers)])].sort()) {
    const p = [{ key: "headers" }, { key: name }];
    const [a, b] = [snapshot.headers[name], fresh.headers[name]];
    if (a !== undefined && b !== undefined) {
      if (a !== b) at(p, "changed", a, b, [true, true]);
    } else if (a !== undefined) at(p, "removed", a, null, [true, false]);
    else at(p, "added", null, b, [false, true]);
  }
  walk([{ key: "body" }], snapshot.body, fresh.body);
  return out;
}

/** The `value` of the assertion for a new snapshot: `users/get.yml` gives `get.snap.json`. */
export function defaultValue(requestPath: string): string {
  const file = requestPath.split("/").pop() ?? requestPath;
  return `${file.replace(/\.yml$/, "")}.snap.json`;
}

/** The companion file of a request for an assertion `value`, relative to the collection. */
export function companionPath(requestPath: string, value: string): string {
  const i = requestPath.lastIndexOf("/");
  return i < 0 ? value : `${requestPath.slice(0, i)}/${value}`;
}

/** The message of a failing snapshot assertion (`rt_core::snapshot::summary`). */
export function summary(differences: SnapshotDiff[]): string {
  const shown = differences.slice(0, 5).map((d) => `${d.path} (${d.expected === null ? "nothing" : JSON.stringify(d.expected)} -> ${d.actual === null ? "nothing" : JSON.stringify(d.actual)})`);
  const more = differences.length - 5;
  return `the response differs from the snapshot at ${differences.length} place(s): ${shown.join("; ")}${more > 0 ? `; and ${more} more` : ""}`;
}
