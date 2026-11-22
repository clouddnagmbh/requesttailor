// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's branch compare and bisect (wave 12, strand C): runs at a revision, comparison per
// request, the halving with skipped commits and cancel.

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { compareSteps, jsonChanges, readMockRequest, searchBisect } from "./mock-compare";
import type { MockGit } from "./mock-git";
import type { BisectVerdict, BodyChange, GitCommit } from "./types";

const input = { environment: "Local", vars: [] as [string, string][] };

async function repo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  const first = await git.revParse("HEAD");
  // The health check asserts 200; from the second commit on it goes to /status/503.
  const spec = await b.getRequest("health.yml");
  await b.updateRequest("health.yml", {
    ...spec,
    assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
  });
  await b.save();
  const second = await git.commit("Assert 200", ["health.yml"]);
  await b.updateRequest("health.yml", { ...(await b.getRequest("health.yml")), url: "{{baseUrl}}/status/503" });
  await b.save();
  const third = await git.commit("Health moved", ["health.yml"]);
  await b.updateRequest("health.yml", { ...(await b.getRequest("health.yml")), name: "Health!" });
  await b.save();
  const fourth = await git.commit("Rename", ["health.yml"]);
  return { b, git, ids: [first, second.oid, third.oid, fourth.oid] };
}

describe("mock compare", () => {
  it("runs a revision from its own files", async () => {
    const { b, ids } = await repo();
    const early = await b.compare.run({ rev: ids[1]! }, input);
    expect(early.commit.oid).toBe(ids[1]);
    const health = early.steps.find((s) => s.request_path === "health.yml")!;
    expect(health.passed).toBe(true);
    expect(health.report.assertions?.[0]?.passed).toBe(true);
    const late = await b.compare.run({ rev: "HEAD" }, input);
    const broken = late.steps.find((s) => s.request_path === "health.yml")!;
    expect(broken.report.response?.status).toBe(503);
    expect(broken.passed).toBe(false);
    expect(late.summary.failed).toBeGreaterThan(0);
  });

  it("compares status, headers, body and tests", async () => {
    const { b, ids } = await repo();
    const cmp = await b.compare.compare({ from: ids[1]!, to: "HEAD" }, input);
    const health = cmp.requests.find((r) => r.path === "health.yml")!;
    expect(health.same).toBe(false);
    expect(health.status).toEqual({ old: 200, new: 503 });
    expect(health.headers).toEqual([{ name: "x-mock-name", old: "Health", new: "Health!" }]);
    expect(health.body.map((c) => c.pointer)).toEqual(["/name", "/url"]);
    expect(health.tests.map((t) => t.name).sort()).toEqual(["res.status eq 200", "status is 2xx"]);
    // Requests that did not change answer the same.
    expect(cmp.requests.filter((r) => r.path !== "health.yml").every((r) => r.same)).toBe(true);
    // A folder limits the requests.
    const none = await b.compare.compare({ from: ids[1]!, to: "HEAD", folder: "nowhere" }, input);
    expect(none.requests).toEqual([]);
  });

  it("bisects to the commit that broke the assertion, reporting every step", async () => {
    const { b, ids } = await repo();
    const seen: string[] = [];
    const outcome = await b.compare.bisect({ good: ids[0]!, bad: "HEAD" }, input, (s) => seen.push(s.commit.oid));
    // The first commit that is bad is the one that moved the URL: the second only asserts.
    expect(outcome.first_bad?.oid).toBe(ids[2]);
    expect(outcome.candidates).toBe(3);
    expect(seen).toEqual(outcome.steps.map((s) => s.commit.oid));
    expect(outcome.steps[0]!.verdict).toBe("bad");
    expect(outcome.steps[0]!.failed[0]).toMatch(/Health.*res\.status eq 200 \(got 503\)/);
  });

  it("refuses a second run while one is in progress", async () => {
    const { b } = await repo();
    const first = b.compare.run({ rev: "HEAD" }, input);
    await expect(b.compare.run({ rev: "HEAD" }, input)).rejects.toThrow(/in progress/);
    await first;
    await b.compare.run({ rev: "HEAD" }, input);
  });

  it("reads request files and diffs JSON by pointer", () => {
    const file = readMockRequest(
      "a.yml",
      'info:\n  name: "A: b"\n  type: http\n  seq: 1\nhttp:\n  method: POST\n  url: "{{base}}/x"\nassertions:\n  - expression: res.status\n    operator: eq\n    value: "201"\n',
    );
    expect(file).toEqual({
      name: "A: b",
      method: "POST",
      url: "{{base}}/x",
      assertions: [{ expression: "res.status", operator: "eq", value: "201" }],
    });
    const out: BodyChange[] = [];
    jsonChanges({ a: 1, "b/c": [1, 2], d: { e: true } }, { a: 2, "b/c": [1], d: { e: true } }, "", out);
    expect(out).toEqual([
      { pointer: "/a", old: 1, new: 2 },
      { pointer: "/b~1c/1", old: 2, new: null },
    ]);
    expect(compareSteps(["x.yml"], [], [])[0]!.same).toBe(true);
  });
});

describe("mock bisect search", () => {
  const commits = (n: number): GitCommit[] =>
    Array.from({ length: n }, (_, k) => ({ oid: `c${n - k}`, summary: "", author: "", time: n - k }));
  const id = (c: GitCommit) => Number(c.oid.slice(1));
  const run = async (n: number, verdict: (i: number) => BisectVerdict, cancelled = () => false) => {
    const tested: number[] = [];
    const outcome = await searchBisect(
      commits(n),
      async (c) => {
        tested.push(id(c));
        return [verdict(id(c)), []];
      },
      () => {},
      cancelled,
    );
    return { outcome, tested };
  };

  it("finds every first bad commit of 49 in at most seven tests", async () => {
    for (let first = 1; first <= 49; first++) {
      const { outcome, tested } = await run(49, (i) => (i >= first ? "bad" : "good"));
      expect(outcome.first_bad?.oid).toBe(`c${first}`);
      expect(tested.length).toBeLessThanOrEqual(7);
    }
  });

  it("handles all good, all bad, skipped and cancelled searches", async () => {
    expect((await run(10, () => "good")).outcome.first_bad).toBeNull();
    expect((await run(10, () => "bad")).outcome.first_bad?.oid).toBe("c1");
    const around = await run(20, (i) => (i >= 9 && i <= 12 ? "skip" : i >= 15 ? "bad" : "good"));
    expect(around.outcome.first_bad?.oid).toBe("c15");
    const only = await run(12, (i) => (i <= 5 ? "good" : i <= 8 ? "skip" : "bad"));
    expect(only.outcome.first_bad).toBeNull();
    let n = 0;
    const cut = await run(30, () => (n++, "bad"), () => n >= 2);
    expect(cut.outcome.first_bad).toBeNull();
    expect(cut.outcome.steps).toHaveLength(1);
  });
});
