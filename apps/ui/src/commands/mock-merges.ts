// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the mock backend (wave 12, strand B), on the mock Git's merge in progress.
//
// The mock Git (./mock-git.ts) merges whole requests: a request both sides changed is
// conflicted. Here the conflicts are recomputed per field, like `rt_app::merges` does from
// the index stages: base (merge base), ours (HEAD) and theirs (the merge head) of each
// conflicted request are compared field by field (field paths as in `rt_format::diff`). A
// field only one side changed merges; both sides differently is a `FieldConflict`. A request
// without field conflicts is in `clean`; a request deleted on one side and changed on the
// other is a text conflict (a file choice). `resolve` writes the merged request and marks
// it resolved; the next commit concludes the merge.
//
// The mock Git keeps its commits private; `revisions` reads them (see the note there).

import { BackendError, type MergeApi } from "./backend";
import type { MockGit } from "./mock-git";
import type { MockWave12Host } from "./mock-wave12";
import type { ConflictChoice, DiffValue, FieldConflict, FieldPath, MergeState, RequestSpec, Side } from "./types";

type Tree = Map<string, RequestSpec>;

/** The commits of the merge in progress. The mock Git has no public accessor for a
 *  revision's specs (only their YAML text), so this reads its private store; the mock is the
 *  only user. */
function revisions(git: MockGit): { base: Tree; ours: Tree; theirs: Tree } | null {
  const g = git as unknown as {
    mergeHead: string | null;
    head(): string | null;
    baseOf(a: string, b: string): string | null;
    store: Map<string, { tree: Tree }>;
  };
  const theirs = g.mergeHead;
  const ours = g.head();
  if (!theirs || !ours) return null;
  const base = g.baseOf(ours, theirs);
  const tree = (oid: string | null): Tree => (oid ? (g.store.get(oid)?.tree ?? new Map()) : new Map());
  return { base: tree(base), ours: tree(ours), theirs: tree(theirs) };
}

const text = (t: string): DiffValue => ({ kind: "text", text: t });
const json = (value: unknown): DiffValue => ({ kind: "json", value });
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);
const keyOf = (path: FieldPath) => JSON.stringify(path);

/** A field of a request: where it is, its raw value and how it is shown. */
interface Field {
  path: FieldPath;
  raw: unknown;
  shown: DiffValue;
}

/** Names list entries by a key; a second entry with the same key gets `#2`. */
function keyed<T>(items: T[], key: (item: T) => string): [string, T][] {
  const seen = new Map<string, number>();
  return items.map((item) => {
    const k = key(item);
    const n = (seen.get(k) ?? 0) + 1;
    seen.set(k, n);
    return [n === 1 ? k : `${k}#${n}`, item];
  });
}

/** List fields of a spec, with the key of each entry (`rt_format::diff`'s segments). */
const LISTS = {
  headers: (s: RequestSpec) => keyed(s.headers ?? [], (h) => h.name),
  params: (s: RequestSpec) => keyed(s.params ?? [], (p) => `${p.kind}\u0000${p.name}`),
  vars: (s: RequestSpec) => keyed(s.vars ?? [], (v) => v.name),
  scripts: (s: RequestSpec) => keyed(s.scripts ?? [], (x) => x.phase),
  assertions: (s: RequestSpec) => keyed(s.assertions ?? [], (a) => a.expression),
  actions: (s: RequestSpec) => keyed(s.actions ?? [], (a) => a.variable),
} as const;
type ListName = keyof typeof LISTS;

const listPath = (name: ListName, key: string): FieldPath =>
  name === "params" ? ["params", ...key.split("\u0000")] : [name, key];

function fields(spec: RequestSpec | undefined): Map<string, Field> {
  const out = new Map<string, Field>();
  if (!spec) return out;
  const put = (path: FieldPath, raw: unknown, shown: DiffValue) => out.set(keyOf(path), { path, raw, shown });
  put(["name"], spec.name, text(spec.name));
  if (spec.seq !== null) put(["seq"], spec.seq, text(String(spec.seq)));
  put(["method"], spec.method, text(spec.method));
  put(["url"], spec.url, text(spec.url));
  put(["body"], spec.body, json(spec.body));
  put(["auth"], spec.auth, json(spec.auth));
  for (const [k, v] of Object.entries(spec.settings ?? {})) put(["settings", k], v, json(v));
  if (spec.protocol) put(["protocol"], spec.protocol, json(spec.protocol));
  for (const name of Object.keys(LISTS) as ListName[]) {
    for (const [k, item] of LISTS[name](spec) as [string, unknown][]) {
      const entry = item as { value?: string | null; secret?: boolean; enabled?: boolean; code?: string };
      const shown: DiffValue = entry.secret
        ? { kind: "secret" }
        : name === "scripts"
          ? text(entry.code ?? "")
          : typeof entry.value === "string" && entry.enabled !== false && name !== "assertions"
            ? text(entry.value)
            : json(item);
      put(listPath(name, k), item, shown);
    }
  }
  return out;
}

interface FileMerge {
  /** The merged request (`null`: deleted); `undefined` while a conflict has no choice. */
  spec: RequestSpec | null | undefined;
  conflicts: FieldConflict[];
  text: boolean;
}

/** Field merge of one request, with `choices` applied where given. */
function mergeSpec(
  path: string,
  b: RequestSpec | undefined,
  o: RequestSpec | undefined,
  t: RequestSpec | undefined,
  choices: ConflictChoice[],
): FileMerge {
  const fileChoice = choices.find((c) => c.kind === "file" && c.path === path)?.side;
  if (!o || !t) {
    // Deleted on one side, changed on the other: a decision for the whole file.
    if (!fileChoice) return { spec: undefined, conflicts: [], text: true };
    return { spec: (fileChoice === "ours" ? o : t) ?? null, conflicts: [], text: true };
  }
  const [fb, fo, ft] = [fields(b), fields(o), fields(t)];
  const target = { kind: "request" as const, path };
  const merged = new Map<string, Field | undefined>();
  const conflicts: FieldConflict[] = [];
  let undecided = false;
  for (const key of new Set([...fo.keys(), ...ft.keys(), ...fb.keys()])) {
    const [vb, vo, vt] = [fb.get(key), fo.get(key), ft.get(key)];
    if (same(vo?.raw, vt?.raw) || same(vb?.raw, vt?.raw)) merged.set(key, vo);
    else if (same(vb?.raw, vo?.raw)) merged.set(key, vt);
    else {
      const field = (vo ?? vt)!.path;
      conflicts.push({ target, file: path, field, base: vb?.shown ?? null, ours: vo?.shown ?? null, theirs: vt?.shown ?? null });
      const side: Side | undefined =
        fileChoice ??
        choices.find((c) => c.kind === "field" && same(c.target, target) && same(c.field, field))?.side;
      if (!side) undecided = true;
      merged.set(key, side === "theirs" ? vt : vo);
    }
  }
  return { spec: undecided ? undefined : build(o, t, merged), conflicts, text: false };
}

/** Ours with the merged fields: scalars set, list entries in ours' order, theirs' additions after. */
function build(o: RequestSpec, t: RequestSpec, merged: Map<string, Field | undefined>): RequestSpec {
  const spec = structuredClone(o);
  const raw = <T>(path: FieldPath) => merged.get(keyOf(path))?.raw as T | undefined;
  spec.name = raw<string>(["name"]) ?? o.name;
  spec.seq = raw<number>(["seq"]) ?? null;
  spec.method = raw<string>(["method"]) ?? o.method;
  spec.url = raw<string>(["url"]) ?? "";
  spec.body = structuredClone(raw<RequestSpec["body"]>(["body"]) ?? o.body);
  spec.auth = structuredClone(raw<RequestSpec["auth"]>(["auth"]) ?? o.auth);
  const settings: Record<string, unknown> = {};
  for (const k of new Set([...Object.keys(o.settings ?? {}), ...Object.keys(t.settings ?? {})])) {
    const v = raw(["settings", k]);
    if (v !== undefined) settings[k] = structuredClone(v);
  }
  spec.settings = settings as unknown as RequestSpec["settings"];
  const protocol = raw<RequestSpec["protocol"]>(["protocol"]);
  if (protocol) spec.protocol = structuredClone(protocol);
  else delete spec.protocol;
  for (const name of Object.keys(LISTS) as ListName[]) {
    const keys = [...new Set([...LISTS[name](o), ...LISTS[name](t)].map(([k]) => k))];
    const items = keys.flatMap((k) => {
      const v = raw(listPath(name, k));
      return v === undefined ? [] : [structuredClone(v)];
    });
    (spec as unknown as Record<string, unknown[]>)[name] = items;
  }
  return spec;
}

export function mockMerges(host: MockWave12Host): MergeApi {
  async function current(): Promise<{ paths: string[]; base: Tree; ours: Tree; theirs: Tree } | null> {
    const status = await host.git().status();
    if (!status.merging) return null;
    const revs = revisions(host.git());
    if (!revs) return null;
    const paths = status.files
      .filter((f) => f.state === "conflicted")
      .map((f) => f.path)
      .sort();
    return { paths, ...revs };
  }

  return {
    async conflicts(): Promise<MergeState> {
      const m = await current();
      if (!m) return { merging: false, conflicts: [], text_conflicts: [] };
      const state: MergeState = { merging: true, conflicts: [], text_conflicts: [] };
      const clean: string[] = [];
      for (const p of m.paths) {
        const f = mergeSpec(p, m.base.get(p), m.ours.get(p), m.theirs.get(p), []);
        if (f.text) state.text_conflicts.push(p);
        else if (f.conflicts.length > 0) state.conflicts.push(...f.conflicts);
        else clean.push(p);
      }
      if (clean.length > 0) state.clean = clean;
      return state;
    },

    async resolve(choices: ConflictChoice[]): Promise<void> {
      const m = await current();
      if (!m) throw new BackendError("git", "No merge is in progress.");
      for (const p of m.paths) {
        const f = mergeSpec(p, m.base.get(p), m.ours.get(p), m.theirs.get(p), choices);
        if (f.spec === undefined) continue;
        // Marks the file resolved (refuses with unsaved changes), then writes the merge.
        await host.git().resolve(p, "ours");
        for (const tree of [host.saved(), host.requests()]) {
          if (f.spec) tree.set(p, structuredClone(f.spec));
          else tree.delete(p);
        }
      }
    },
  };
}
