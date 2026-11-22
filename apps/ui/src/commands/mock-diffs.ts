// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the mock backend (wave 12, strand A): a TypeScript counterpart of
// `rt_format::diff` for the mock's requests (specs in memory, no YAML), good enough for the
// UI tests and `?backend=mock`: the same field paths, kinds and masking, requests only (the
// mock has no folders, environments in commits or collection files). `host` (./mock-wave12.ts)
// gives the mock's Git and requests; read it only inside the methods.

import type { DiffApi } from "./backend";
import { renderSpec, type MockGit } from "./mock-git";
import type { MockWave12Host } from "./mock-wave12";
import type {
  ChangedFile,
  CollectionDiff,
  DiffValue,
  FieldChange,
  FieldPath,
  ModelChange,
  RequestSpec,
} from "./types";

interface Field {
  path: FieldPath;
  value: DiffValue;
  secret: boolean;
}

const text = (path: FieldPath, value: string, secret = false): Field => ({ path, value: { kind: "text", text: value }, secret });

function flatten(out: Field[], prefix: FieldPath, value: unknown): void {
  if (value === null || typeof value !== "object") return;
  for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
    if (v === null || v === undefined) continue;
    out.push(typeof v === "string" ? text([...prefix, k], v) : { path: [...prefix, k], value: { kind: "json", value: v }, secret: false });
  }
}

/** Entries of a list by key; a repeated key gets `#2`, `#3`, ... */
function keyed<T>(list: T[], key: (t: T) => string): [string, T][] {
  const seen = new Map<string, number>();
  return list.map((item) => {
    const k = key(item);
    const n = (seen.get(k) ?? 0) + 1;
    seen.set(k, n);
    return [n === 1 ? k : `${k}#${n}`, item];
  });
}

function fieldsOf(s: RequestSpec): Field[] {
  const out: Field[] = [text(["name"], s.name)];
  if (s.seq !== null) out.push(text(["seq"], String(s.seq)));
  out.push(text(["method"], s.method), text(["url"], s.url));
  const entry = (prefix: FieldPath, key: string, value: string, enabled: boolean, secret = false) => {
    out.push(text([...prefix, key], value, secret), text([...prefix, key, "@enabled"], String(enabled)));
  };
  for (const [k, h] of keyed(s.headers, (h) => h.name)) entry(["headers"], k, h.value, h.enabled);
  for (const [k, p] of keyed(s.params, (p) => `${p.kind}\u0000${p.name}`)) entry(["params", p.kind], k.split("\u0000")[1], p.value, p.enabled);
  if (s.body.type !== "none") flatten(out, ["body"], s.body);
  if (s.auth.type !== "inherit") flatten(out, ["auth"], s.auth);
  for (const [k, v] of keyed(s.vars, (v) => v.name)) {
    entry(["vars"], k, v.value ?? "", v.enabled, v.secret);
    if (v.secret) out.push(text(["vars", k, "@secret"], "true"));
  }
  flatten(out, ["settings"], s.settings);
  for (const [k, sc] of keyed(s.scripts, (sc) => sc.phase)) out.push(text(["scripts", k], sc.code));
  for (const [k, a] of keyed(s.assertions, (a) => a.expression)) {
    const { expression: _e, ...rest } = a;
    out.push({ path: ["assertions", k], value: { kind: "json", value: rest }, secret: false });
  }
  for (const [k, a] of keyed(s.actions, (a) => a.variable)) {
    const { variable: _v, ...rest } = a;
    out.push({ path: ["actions", k], value: { kind: "json", value: rest }, secret: false });
  }
  return out;
}

const eq = (a: Field, b: Field) => a.secret === b.secret && JSON.stringify(a.value) === JSON.stringify(b.value);

function compare(old: Field[], next: Field[]): FieldChange[] {
  const at = (list: Field[]) => new Map(list.map((f) => [f.path.join("\u0000"), f]));
  const [o, n] = [at(old), at(next)];
  const shown = (f: Field, hide: boolean): DiffValue => (hide ? { kind: "secret" } : f.value);
  const out: FieldChange[] = [];
  for (const f of old) {
    const g = n.get(f.path.join("\u0000"));
    if (!g) out.push({ path: f.path, old: shown(f, f.secret), new: null });
    else if (!eq(f, g)) out.push({ path: f.path, old: shown(f, f.secret || g.secret), new: shown(g, f.secret || g.secret) });
  }
  for (const g of next) if (!o.has(g.path.join("\u0000"))) out.push({ path: g.path, old: null, new: shown(g, g.secret) });
  // A flag of an entry that came or went whole is not a change of its own while it is on.
  const whole = new Set(out.filter((c) => !c.old || !c.new).map((c) => c.path.join("\u0000")));
  const on = (v: DiffValue | null) => v?.kind === "text" && v.text === "true";
  const kept = out.filter((c) => {
    const last = c.path[c.path.length - 1];
    if (last !== "@enabled" && last !== "@secret") return true;
    return !(whole.has(c.path.slice(0, -1).join("\u0000")) && (on(c.old) || on(c.new)));
  });
  const order = ["name", "seq", "method", "url", "headers", "params", "body", "auth", "vars", "settings", "scripts", "assertions", "actions"];
  const rank = (c: FieldChange) => (order.indexOf(c.path[0]) + order.length + 1) % (order.length + 1);
  return kept.sort((a, b) => rank(a) - rank(b));
}

/** The 1-based line of `path` in the rendered request, or the section's line. */
function lineOf(rendered: string, path: FieldPath): number | undefined {
  const lines = rendered.split("\n");
  const section = path[0];
  const start = lines.findIndex((l) => new RegExp(`^\\s*${section}:`).test(l));
  if (start < 0) return undefined;
  let at = start;
  for (const seg of path.slice(1)) {
    if (seg.startsWith("@")) break;
    const name = seg.split("#")[0];
    const hit = lines.slice(at).findIndex((l) => l.includes(name));
    if (hit < 0) break;
    at += hit;
  }
  return at + 1;
}

const seqless = (s: RequestSpec) => JSON.stringify({ ...s, seq: null });

type Specs = Map<string, RequestSpec>;

/** Added, removed, moved and modified requests of `old` → `next`. */
export function diffSpecs(old: Specs, next: Specs): CollectionDiff {
  const changes: ModelChange[] = [];
  const files: ChangedFile[] = [];
  const modified = (path: string, o: RequestSpec, n: RequestSpec) => {
    const rendered = renderSpec(n);
    const fields = compare(fieldsOf(o), fieldsOf(n)).map((f) => ({ ...f, line: lineOf(f.new ? rendered : renderSpec(o), f.path) }));
    return { fields, same: renderSpec(o) === rendered, path };
  };
  const removed = [...old].filter(([p]) => !next.has(p));
  const added = [...next].filter(([p]) => !old.has(p));
  for (const [path, n] of next) {
    const o = old.get(path);
    if (!o) continue;
    const m = modified(path, o, n);
    if (m.same) continue;
    files.push({ path, status: "modified", covered: true });
    if (m.fields.length > 0) changes.push({ target: { kind: "request", path }, change: { kind: "modified" }, file: path, name: n.name, fields: m.fields });
  }
  const used = new Set<string>();
  for (const [from, o] of removed) {
    const hit =
      added.find(([p, n]) => !used.has(p) && seqless(n) === seqless(o)) ??
      added.find(([p, n]) => !used.has(p) && n.name === o.name && removed.filter(([, r]) => r.name === o.name).length === 1 && added.filter(([, a]) => a.name === o.name).length === 1);
    if (hit) {
      used.add(hit[0]);
      const m = modified(hit[0], o, hit[1]);
      changes.push({ target: { kind: "request", path: hit[0] }, change: { kind: "moved", from }, file: hit[0], name: hit[1].name, fields: m.fields });
      files.push({ path: from, status: "removed", covered: true }, { path: hit[0], status: "added", covered: true });
    } else {
      changes.push({ target: { kind: "request", path: from }, change: { kind: "removed" }, file: from, name: o.name, fields: [] });
      files.push({ path: from, status: "removed", covered: true });
    }
  }
  for (const [path, n] of added) {
    if (used.has(path)) continue;
    changes.push({ target: { kind: "request", path }, change: { kind: "added" }, file: path, name: n.name, fields: [] });
    files.push({ path, status: "added", covered: true });
  }
  files.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  return { changes, files };
}

export function mockDiffs(host: MockWave12Host): DiffApi {
  /** The requests of a commit; no commit (or an unknown rev) is an empty collection. */
  const specsAt = async (rev: string): Promise<Specs> => {
    const git = host.git();
    let oid: string;
    try {
      oid = await git.revParse(rev);
    } catch {
      return new Map();
    }
    // The mock keeps each commit's requests as specs, which a text tree cannot give back.
    const store = (git as unknown as { store: Map<string, { tree: Specs }> }).store;
    return structuredClone(store.get(oid)?.tree ?? new Map());
  };
  const known = async (rev: string): Promise<Specs> => {
    const git: MockGit = host.git();
    await git.revParse(rev);
    return specsAt(rev);
  };
  return {
    working: async () => diffSpecs(await specsAt("HEAD"), new Map(host.saved())),
    commit: async (rev) => diffSpecs(await specsAt(`${rev}^`), await known(rev)),
    between: async (from, to) => diffSpecs(await known(from), await known(to)),
  };
}
