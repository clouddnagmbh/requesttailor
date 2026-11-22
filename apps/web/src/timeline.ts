// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `TimelineApi` of the web app (wave 12, strand D), and the recorder every run of ./runner.ts
// reports to. The file history comes from the web Git (`fileLog` follows renames, `show`), the
// field diff, blame and the results records from the core (`timeline*` of rt-wasm, i.e.
// `rt_app::timeline`, the same code as the desktop app). Results of a run of `HEAD`'s files
// are kept per commit in IndexedDB, never in the repository: nothing unsaved and every file of
// the collection folder equal to `HEAD`'s.

import {
  BackendError,
  type FieldBlame,
  type GitFileCommit,
  type RecordSource,
  type RequestSpec,
  type RequestTimeline,
  type RunEvent,
  type RunStep,
  type TestRecord,
  type TimelineApi,
} from "@requesttailor/ui";

import { workingFiles } from "./diffs";
import { toBackendError } from "./wasm";
import type { Wave12Host } from "./wave12";
import type { Session } from "./web-backend";

/** The timeline's part of the rt-wasm module. */
export interface CoreTimelineModule {
  timelineBuild(
    path: string,
    collection: string,
    versions: ProbedVersion[],
    records: Record<string, TestRecord[]>,
  ): RequestTimeline;
  timelineBlame(versions: ProbedVersion[]): FieldBlame[];
  timelineSpec(text: string): RequestSpec;
  timelineMergeRecords(existing: string, records: TestRecord[]): string;
  timelineParseRecords(text: string): TestRecord[];
  timelineRecordOfStep(collection: string, time: number, source: RecordSource, step: RunStep): TestRecord;
}

/** A file log entry and the file's text at that commit (`null`: deleted there). */
export interface ProbedVersion {
  commit: GitFileCommit;
  text: string | null;
}

/** The recorded results of one commit: the text of its results file (one JSON line per
 *  record), by commit id. */
export interface ResultsStore {
  get(oid: string): Promise<string>;
  set(oid: string, text: string): Promise<void>;
}

export function memoryResultsStore(): ResultsStore {
  const map = new Map<string, string>();
  return {
    get: async (oid) => map.get(oid) ?? "",
    set: async (oid, text) => void map.set(oid, text),
  };
}

const DB_NAME = "requesttailor-test-results";
const OBJECTS = "results";

function request<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error ?? new Error("IndexedDB failed"));
  });
}

/** The results in the browser's IndexedDB (one record per commit id). Where IndexedDB is not
 *  there, the store keeps nothing and reads empty. */
export function indexedDbResultsStore(): ResultsStore {
  let db: Promise<IDBDatabase | null> | null = null;
  const open = (): Promise<IDBDatabase | null> => {
    db ??= new Promise((resolve) => {
      try {
        const req = indexedDB.open(DB_NAME, 1);
        req.onupgradeneeded = () => req.result.createObjectStore(OBJECTS);
        req.onsuccess = () => resolve(req.result);
        req.onerror = () => resolve(null);
      } catch {
        resolve(null);
      }
    });
    return db;
  };
  return {
    get: async (oid) => {
      const d = await open();
      if (!d) return "";
      const value: unknown = await request(d.transaction(OBJECTS).objectStore(OBJECTS).get(oid));
      return typeof value === "string" ? value : "";
    },
    set: async (oid, text) => {
      const d = await open();
      if (d) await request(d.transaction(OBJECTS, "readwrite").objectStore(OBJECTS).put(text, oid));
    },
  };
}

/** Versions kept in a blame; older history is ignored. */
const BLAME_LIMIT = 500;

/** The collection key of records: the web app's collection is the repository root. */
const COLLECTION = "";

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

/** What the recorder needs of the running backend (one per page). */
let current: { w: Wave12Host; store: ResultsStore } | null = null;

export function webTimeline(w: Wave12Host, store: ResultsStore = indexedDbResultsStore()): TimelineApi {
  current = { w, store };
  const core = async () => (await w.host.deps.loadCore()) as unknown as CoreTimelineModule;
  const versions = async (path: string, limit: number): Promise<ProbedVersion[]> => {
    const log = await w.git.fileLog(path, limit);
    const out: ProbedVersion[] = [];
    for (const commit of log) {
      let text: string | null = null;
      if (commit.change !== "deleted") {
        // A version that is no text file has no readable fields.
        text = await w.git.show(commit.commit.oid, commit.path).catch(() => null);
      }
      out.push({ commit, text });
    }
    return out;
  };
  const recordsOf = async (c: CoreTimelineModule, oid: string) => c.timelineParseRecords(await store.get(oid));
  return {
    history: (path, limit) =>
      guard(async () => {
        const list = await versions(path, limit);
        const c = await core();
        const records: Record<string, TestRecord[]> = {};
        for (const v of list) records[v.commit.commit.oid] = await recordsOf(c, v.commit.commit.oid);
        return c.timelineBuild(path, COLLECTION, list, records);
      }),
    blame: (path) =>
      guard(async () => {
        const list = await versions(path, BLAME_LIMIT);
        return (await core()).timelineBlame(list);
      }),
    version: (rev, path) =>
      guard(async () => {
        const text = await w.git.show(rev, path);
        if (text === null) throw new BackendError("git", `${path} does not exist at ${rev}.`);
        return (await core()).timelineSpec(text);
      }),
    results: (rev) =>
      guard(async () => {
        const oid = await w.git.revParse(rev);
        const all = await recordsOf(await core(), oid);
        return all.filter((r) => r.collection === COLLECTION).sort((a, b) => b.time - a.time);
      }),
  };
}

/** Whether the saved collection folder has exactly the files of `HEAD`; `null` if not (or
 *  if there is no commit), else `HEAD`'s id. */
async function headIfClean(w: Wave12Host, session: Session): Promise<string | null> {
  const ws = session.ws as unknown as { dirty(): string[] };
  if (ws.dirty().length > 0) return null;
  try {
    const head = await w.git.treeAt("HEAD");
    const working = new Map(await workingFiles(session.handle));
    const files = Object.entries(head.files);
    if (working.size !== files.length) return null;
    return files.every(([path, text]) => working.get(path) === text) ? head.oid : null;
  } catch {
    return null;
  }
}

/** Watches one run of the open collection; created before the run starts. When the run ends
 *  it keeps the finished requests under `HEAD` if the run used exactly `HEAD`'s files. */
export function runRecorder(session: Session): (event: RunEvent) => void {
  const now = current;
  if (!now) return () => {};
  const { w, store } = now;
  const clean = headIfClean(w, session);
  const steps: RunStep[] = [];
  return (event) => {
    if (event.kind === "step" && !event.step.skipped) steps.push(event.step);
    if (event.kind !== "finished") return;
    // Recording is a convenience: it never fails the run.
    void (async () => {
      const oid = await clean;
      if (!oid || steps.length === 0) return;
      const core = (await w.host.deps.loadCore()) as unknown as CoreTimelineModule;
      const time = Math.floor(Date.now() / 1000);
      const records = steps.map((s) => core.timelineRecordOfStep(COLLECTION, time, "runner", s));
      await store.set(oid, core.timelineMergeRecords(await store.get(oid), records));
    })().catch(() => undefined);
  };
}
