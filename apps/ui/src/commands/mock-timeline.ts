// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the mock backend (wave 12, strand D): the history, blame and old versions of
// a request from the mock Git's commits (specs in memory, no YAML: the field paths and masking
// follow `rt_format::diff`, requests only), and test results per commit. The mock's runner does
// not record runs; tests and demos add results with `record`. `host` (./mock-wave12.ts) gives
// the mock's Git; read it only inside the methods.

import type { TimelineApi } from "./backend";
import { BackendError } from "./backend";
import type { MockGit } from "./mock-git";
import type { MockWave12Host } from "./mock-wave12";
import type {
  DiffValue,
  FieldBlame,
  FieldChange,
  FieldPath,
  GitFileCommit,
  RecordSource,
  RecordedTest,
  RequestSpec,
  RequestTimeline,
  TestRecord,
} from "./types";

/** At most this many records per request and commit, as in `rt_app::timeline`. */
const RECORDS_PER_REQUEST = 20;

const text = (value: string): DiffValue => ({ kind: "text", text: value });

/** The fields a request has, by path, in model order (`rt_format::diff` names them). */
function fieldsOf(s: RequestSpec): Map<string, { path: FieldPath; value: DiffValue }> {
  const out = new Map<string, { path: FieldPath; value: DiffValue }>();
  const put = (path: FieldPath, value: DiffValue) => out.set(path.join("\u0000"), { path, value });
  const flatten = (prefix: FieldPath, value: object) => {
    for (const [k, v] of Object.entries(value as Record<string, unknown>)) {
      if (v === null || v === undefined) continue;
      put([...prefix, k], typeof v === "string" ? text(v) : { kind: "json", value: v });
    }
  };
  put(["name"], text(s.name));
  put(["method"], text(s.method));
  put(["url"], text(s.url));
  const seen = new Map<string, number>();
  /** A repeated name of one list gets `#2`, `#3`, ... */
  const unique = (list: string, name: string) => {
    const n = (seen.get(`${list}\u0000${name}`) ?? 0) + 1;
    seen.set(`${list}\u0000${name}`, n);
    return n === 1 ? name : `${name}#${n}`;
  };
  for (const h of s.headers) put(["headers", unique("headers", h.name)], text(h.value));
  for (const p of s.params) put(["params", p.kind, unique(`params ${p.kind}`, p.name)], text(p.value));
  if (s.body.type !== "none") flatten(["body"], s.body);
  if (s.auth.type !== "inherit") flatten(["auth"], s.auth);
  for (const v of s.vars) put(["vars", v.name], v.secret ? { kind: "secret" } : text(v.value ?? ""));
  flatten(["settings"], s.settings);
  for (const sc of s.scripts) put(["scripts", sc.phase], text(sc.code));
  for (const a of s.assertions) {
    const { expression, ...rest } = a;
    put(["assertions", expression], { kind: "json", value: rest });
  }
  return out;
}

const sameValue = (a: DiffValue, b: DiffValue) => JSON.stringify(a) === JSON.stringify(b);

/** What `next` changes against `old` (`null`: an empty request). */
function changes(old: RequestSpec | null, next: RequestSpec): FieldChange[] {
  const before = old ? fieldsOf(old) : new Map<string, { path: FieldPath; value: DiffValue }>();
  const out: FieldChange[] = [];
  for (const [key, now] of fieldsOf(next)) {
    const was = before.get(key);
    if (!was) out.push({ path: now.path, old: null, new: now.value });
    else if (!sameValue(was.value, now.value)) out.push({ path: now.path, old: was.value, new: now.value });
  }
  for (const [key, was] of before) {
    if (!fieldsOf(next).has(key)) out.push({ path: was.path, old: was.value, new: null });
  }
  return out;
}

export interface MockTimeline extends TimelineApi {
  /** Test helper: records a result of `request` under the commit `rev` (a run of that commit). */
  record(rev: string, record: Partial<TestRecord> & { request: string; tests?: RecordedTest[] }): Promise<void>;
}

export function mockTimeline(host: MockWave12Host): MockTimeline {
  const records = new Map<string, TestRecord[]>();
  const specAt = (oid: string, path: string): RequestSpec | null => host.git().specAt(oid, path);
  const versions = async (path: string, limit: number): Promise<{ entry: GitFileCommit; spec: RequestSpec | null }[]> => {
    const git: MockGit = host.git();
    return (await git.fileLog(path, limit)).map((entry) => ({
      entry,
      spec: entry.change === "deleted" ? null : specAt(entry.commit.oid, entry.path),
    }));
  };
  const fieldsAt = (list: { spec: RequestSpec | null }[], at: number): FieldChange[] => {
    const spec = list[at].spec;
    return spec ? changes(list[at + 1]?.spec ?? null, spec) : [];
  };
  return {
    history: async (path, limit): Promise<RequestTimeline> => {
      const list = await versions(path, limit);
      return {
        path,
        entries: list.map(({ entry }, at) => ({
          commit: entry,
          fields: fieldsAt(list, at),
          tests: (records.get(entry.commit.oid) ?? [])
            .filter((r) => r.request === entry.path)
            .sort((a, b) => b.time - a.time),
        })),
      };
    },
    blame: async (path): Promise<FieldBlame[]> => {
      const list = await versions(path, 500);
      const head = list[0]?.spec;
      if (!head) return [];
      const per = list.map((_, at) => fieldsAt(list, at));
      return changes(null, head).flatMap((f) => {
        if (!f.new) return [];
        const key = f.path.join("\u0000");
        const at = per.findIndex((fields) => fields.some((c) => c.path.join("\u0000") === key));
        return [{ field: f.path, commit: list[at < 0 ? list.length - 1 : at].entry.commit, value: f.new }];
      });
    },
    version: async (rev, path) => {
      const oid = await host.git().revParse(rev);
      const spec = specAt(oid, path);
      if (!spec) throw new BackendError("git", `${path} does not exist at ${rev}.`);
      return spec;
    },
    results: async (rev) => {
      const oid = await host.git().revParse(rev);
      return [...(records.get(oid) ?? [])].sort((a, b) => b.time - a.time);
    },
    record: async (rev, partial) => {
      const oid = await host.git().revParse(rev);
      const tests = partial.tests ?? [{ name: "status is 200", passed: true }];
      const record: TestRecord = {
        collection: "",
        environment: null,
        time: Math.floor(Date.now() / 1000),
        source: "runner" as RecordSource,
        passed: tests.every((t) => t.passed),
        status: 200,
        ...partial,
        tests,
      };
      const all = [...(records.get(oid) ?? []), record];
      const mine = all.filter((r) => r.request === record.request);
      const drop = new Set(mine.slice(0, Math.max(0, mine.length - RECORDS_PER_REQUEST)));
      records.set(oid, all.filter((r) => !drop.has(r)));
    },
  };
}
