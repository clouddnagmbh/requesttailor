// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's pull-request review (wave 12, strand E): host and token rules, the opened PR as a
// commit the diff and the compare run can read, comments on lines.

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { MOCK_REVIEW_TOKEN } from "./mock-review";

async function ready() {
  const b = new MockBackend();
  await b.openCollection();
  return b;
}

const HOST = { host: "git.example.com", kind: "github" as const, api: "https://git.example.com/api/v3" };

describe("mock review", () => {
  it("knows the host only after it is added, and needs the right token", async () => {
    const b = await ready();
    expect(await b.review.repo()).toBeNull();
    await expect(b.review.list()).rejects.toMatchObject({ kind: "not-found" });
    await expect(b.review.setHosts([{ ...HOST, api: "nope" }])).rejects.toMatchObject({ kind: "host" });
    await b.review.setHosts([HOST]);
    expect(await b.review.repo()).toMatchObject({ host: "git.example.com", project: "acme/demo-api" });
    await expect(b.review.list()).rejects.toMatchObject({ kind: "no-token" });
    await b.review.setToken("git.example.com", "wrong");
    await expect(b.review.list()).rejects.toMatchObject({ kind: "auth" });
    await b.review.setToken("git.example.com", MOCK_REVIEW_TOKEN);
    expect(await b.review.hasToken("git.example.com")).toBe(true);
    expect((await b.review.list()).map((p) => p.number)).toEqual([1, 2]);
    await b.review.deleteToken("git.example.com");
    expect(await b.review.hasToken("git.example.com")).toBe(false);
  });

  it("opens a PR as a commit: its diff, its run, its comments", async () => {
    const b = await ready();
    await b.review.setHosts([HOST]);
    await b.review.setToken("git.example.com", MOCK_REVIEW_TOKEN);
    const session = await b.review.open(1);
    expect(session.pull.number).toBe(1);
    const diff = await b.diffs.between(session.merge_base, session.head_ref);
    const byPath = (p: string) => diff.changes.find((c) => c.target.kind === "request" && c.target.path === p);
    expect(byPath("health.yml")?.change.kind).toBe("modified");
    expect(byPath("ping.yml")?.change.kind).toBe("added");
    const run = await b.compare.run({ rev: session.head_ref, paths: ["health.yml", "ping.yml"] }, { environment: "Local", vars: [] });
    expect(run.steps.map((s) => s.request_path)).toEqual(["health.yml", "ping.yml"]);
    // Opening again is the same session.
    expect((await b.review.open(1)).head_ref).toBe(session.head_ref);

    const comments = await b.review.comments(1);
    expect(comments.some((c) => c.path === null)).toBe(true);
    const onField = comments.find((c) => c.path !== null)!;
    expect(onField.field).toBeDefined();
    const made = await b.review.comment(1, { path: onField.path!, line: onField.line!, body: "Fixed?" });
    expect((await b.review.comments(1)).map((c) => c.body)).toContain("Fixed?");
    expect(made.author).toBe("you");
    await expect(b.review.comment(1, { path: "nope.yml", line: 1, body: "x" })).rejects.toMatchObject({ kind: "host" });

    await b.review.submit(1, { verdict: "approve", body: "" });
    await expect(b.review.submit(1, { verdict: "request-changes", body: " " })).rejects.toMatchObject({ kind: "host" });
    expect((await b.review.ci(1)).state).toBe("failure");
    await expect(b.review.open(99)).rejects.toMatchObject({ kind: "not-found" });
  });
});
