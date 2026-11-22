// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web shell's part of the request history (wave 12, strand D): which versions it reads from
// Git, where the results of a run are kept (one results text per commit, in an in-memory
// store here; IndexedDB in the browser) and when a run is recorded at all. The field diff,
// blame and the records' rules are the core's (`rt_app::timeline`, tested in Rust); here a
// stand-in core shows what the shell hands over.

import type { GitApi, GitFileCommit, RunEvent, RunStep, TestRecord } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { fill, memoryFolder } from "./git/fake-handle.test-util";
import { memoryResultsStore, runRecorder, webTimeline, type CoreTimelineModule, type ProbedVersion } from "./timeline";
import type { Wave12Host } from "./wave12";
import type { Session } from "./web-backend";

const commit = (oid: string, path: string, change: GitFileCommit["change"], from?: string): GitFileCommit => ({
  commit: { oid, summary: `commit ${oid}`, author: "Ada", time: 1 },
  path,
  change,
  ...(from ? { from } : {}),
});

const log: GitFileCommit[] = [
  commit("c3", "api/list.yml", "modified"),
  commit("c2", "api/list.yml", "renamed", "users/get.yml"),
  commit("c1", "users/get.yml", "added"),
];
const texts: Record<string, string> = { "c3:api/list.yml": "v3", "c2:api/list.yml": "v2", "c1:users/get.yml": "v1" };

function fakeCore(): CoreTimelineModule & { built: unknown[]; seen: ProbedVersion[][] } {
  const built: unknown[] = [];
  const seen: ProbedVersion[][] = [];
  return {
    built,
    seen,
    timelineBuild: (path, collection, versions, records) => {
      built.push({ path, collection, records });
      seen.push(versions);
      return { path, entries: [] };
    },
    timelineBlame: (versions) => {
      seen.push(versions);
      return [];
    },
    timelineSpec: (text) => ({ name: text }) as never,
    timelineParseRecords: (text) => text.split("\n").filter(Boolean).map((l) => JSON.parse(l) as TestRecord),
    timelineMergeRecords: (existing, records) =>
      existing + records.map((r) => JSON.stringify(r)).join("\n") + "\n",
    timelineRecordOfStep: (collection, time, source, step) => ({
      collection,
      request: step.request_path,
      environment: null,
      time,
      source,
      passed: step.passed,
      status: null,
      tests: [],
    }),
  };
}

const HEAD = "c3";

function host(core: CoreTimelineModule, files: Record<string, string>, dirty: string[] = []) {
  const handle = memoryFolder();
  const git = {
    fileLog: async () => log,
    show: async (rev: string, path: string) => texts[`${rev}:${path}`] ?? null,
    revParse: async (rev: string) => (rev === "HEAD" ? HEAD : rev),
    treeAt: async () => ({ oid: HEAD, files }),
  } as unknown as GitApi;
  const session = { handle, ws: { dirty: () => dirty } } as unknown as Session;
  const w = { host: { deps: { loadCore: async () => core }, session: () => session }, git } as unknown as Wave12Host;
  return { w, session, handle };
}

function step(path: string, passed: boolean, skipped = false): RunEvent {
  return { kind: "step", step: { request_path: path, passed, skipped } as unknown as RunStep };
}
const finished = { kind: "finished", summary: {} } as unknown as RunEvent;

async function settle() {
  await new Promise((r) => setTimeout(r, 10));
}

describe("webTimeline", () => {
  it("reads every version under the path it had and hands the records of its commits to the core", async () => {
    const core = fakeCore();
    const { w } = host(core, {});
    const store = memoryResultsStore();
    await store.set("c3", `${JSON.stringify({ collection: "", request: "api/list.yml", time: 5 })}\n`);
    const api = webTimeline(w, store);
    await api.history("api/list.yml", 20);
    expect(core.seen[0].map((v) => [v.commit.commit.oid, v.text])).toEqual([
      ["c3", "v3"],
      ["c2", "v2"],
      ["c1", "v1"],
    ]);
    const built = core.built[0] as { path: string; collection: string; records: Record<string, TestRecord[]> };
    expect(built.path).toBe("api/list.yml");
    expect(built.records.c3).toHaveLength(1);
    expect(built.records.c2).toEqual([]);
    expect((await api.version("c1", "users/get.yml")).name).toBe("v1");
    await expect(api.version("c1", "api/list.yml")).rejects.toThrow(/does not exist/);
    const results = await api.results("HEAD");
    expect(results.map((r) => r.time)).toEqual([5]);
  });
});

describe("runRecorder", () => {
  const files = { "a.yml": "one", "sub/b.yml": "two" };

  it("records a run of HEAD's files under HEAD, merged with earlier results", async () => {
    const core = fakeCore();
    const { w, session, handle } = host(core, files);
    await fill(handle, files);
    const store = memoryResultsStore();
    webTimeline(w, store);
    const record = runRecorder(session);
    record({ kind: "started", total: 2 } as RunEvent);
    record(step("a.yml", true));
    record(step("skipped.yml", true, true));
    record(finished);
    await settle();
    const rows = core.timelineParseRecords(await store.get(HEAD));
    expect(rows.map((r) => [r.request, r.passed])).toEqual([["a.yml", true]]);
    const again = runRecorder(session);
    again(step("a.yml", false));
    again(finished);
    await settle();
    expect(core.timelineParseRecords(await store.get(HEAD))).toHaveLength(2);
  });

  it("records nothing for unsaved changes", async () => {
    const core = fakeCore();
    const { w, session, handle } = host(core, files, ["a.yml"]);
    await fill(handle, files);
    const store = memoryResultsStore();
    webTimeline(w, store);
    const record = runRecorder(session);
    record(step("a.yml", true));
    record(finished);
    await settle();
    expect(await store.get(HEAD)).toBe("");
  });

  it("records nothing when the folder differs from HEAD (changed, new or missing file)", async () => {
    for (const disk of [
      { ...files, "a.yml": "changed" },
      { ...files, "new.yml": "x" },
      { "a.yml": "one" },
    ]) {
      const core = fakeCore();
      const { w, session, handle } = host(core, files);
      await fill(handle, disk);
      const store = memoryResultsStore();
      webTimeline(w, store);
      const record = runRecorder(session);
      record(step("a.yml", true));
      record(finished);
      await settle();
      expect(await store.get(HEAD)).toBe("");
    }
  });
});
