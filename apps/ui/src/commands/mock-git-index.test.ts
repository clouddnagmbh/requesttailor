// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand F: line selection (the same cases as crates/rt-git/src/patch.rs) and the
// mock's index (the rules of crates/rt-git/tests/{index_ops,patch}.rs).

import { describe, expect, it } from "vitest";

import { applyLineSelection, chosenLines, splitLines } from "./line-selection";
import { MockBackend } from "./mock";
import { lineDiff, type MockGit } from "./mock-git";
import { MOCK_GIT_CONFIG } from "./mock-git";
import type { GitPatchRequest } from "./types";

describe("line selection", () => {
  const old = "a\nb\nc\n";
  const next = "a\nB\nc\nd\n";
  const diff = lineDiff("x", old, next);
  // Lines: a (context), -b, +B, c (context), +d.
  it("a whole hunk applies all its changes and taking it back restores the old text", () => {
    const all = chosenLines(diff, [{ hunk: 0 }]);
    expect(applyLineSelection(old, next, diff, all)).toBe(next);
    expect(applyLineSelection(old, next, diff, all, true)).toBe(old);
  });

  it("chosen lines only", () => {
    expect(applyLineSelection(old, next, diff, chosenLines(diff, [{ hunk: 0, lines: [2] }]))).toBe("a\nb\nB\nc\n");
    expect(applyLineSelection(old, next, diff, chosenLines(diff, [{ hunk: 0, lines: [1] }]))).toBe("a\nc\n");
    // Taking back "+B" only: the unchosen removal and addition stay.
    expect(applyLineSelection(old, next, diff, chosenLines(diff, [{ hunk: 0, lines: [2] }]), true)).toBe("a\nc\nd\n");
  });

  it("works over several hunks and keeps the lines between them", () => {
    const long = Array.from({ length: 30 }, (_, i) => `l${String(i + 1).padStart(2, "0")}\n`).join("");
    const edited = long.replace("l01\n", "L01\n").replace("l30\n", "L30\n");
    const d = lineDiff("long", long, edited);
    expect(d.hunks).toHaveLength(2);
    expect(applyLineSelection(long, edited, d, chosenLines(d, [{ hunk: 1 }]))).toBe(long.replace("l30\n", "L30\n"));
    expect(applyLineSelection(long, edited, d, chosenLines(d, [{ hunk: 0 }]), true)).toBe(long.replace("l30\n", "L30\n"));
  });

  it("refuses indexes that do not exist and ignores context lines", () => {
    expect(() => chosenLines(diff, [{ hunk: 3 }])).toThrow();
    expect(() => chosenLines(diff, [{ hunk: 0, lines: [99] }])).toThrow();
    expect(chosenLines(diff, [{ hunk: 0, lines: [0, 3] }]).size).toBe(0);
  });

  it("keeps line endings and a missing last line break", () => {
    expect(splitLines("a\r\nb")).toEqual(["a\r\n", "b"]);
    const d = lineDiff("x", "a\r\nb\r\n", "a\r\nB\r\n");
    expect(applyLineSelection("a\r\nb\r\n", "a\r\nB\r\n", d, chosenLines(d, [{ hunk: 0 }]))).toBe("a\r\nB\r\n");
  });
});

async function repo() {
  const b = new MockBackend();
  await b.openCollection();
  const git = b.git as MockGit;
  git.scenario("clean");
  await git.setIdentity({ name: "Ada", email: "ada@example.com" });
  return { b, git };
}

const request = (d: { side: "unstaged" | "staged"; old: string; new: string }, hunks: GitPatchRequest["hunks"]): GitPatchRequest => ({
  path: "/README.md",
  side: d.side,
  old: d.old,
  new: d.new,
  hunks,
});

describe("the mock's index", () => {
  it("stages, unstages and refuses stale selections like the core", async () => {
    const { git } = await repo();
    const d = await git.indexDiff("/README.md", "unstaged");
    expect(d.diff.hunks).toHaveLength(1);
    const lines = d.diff.hunks[0].lines;
    const added = lines.findIndex((l) => l.kind === "added");
    await git.stagePatch(request(d, [{ hunk: 0, lines: [added] }]));
    const staged = await git.indexDiff("/README.md", "staged");
    expect(staged.diff.hunks[0].lines.filter((l) => l.kind === "added")).toHaveLength(1);
    // The unstaged side now shows the rest, with new ids: the old selection is stale.
    await expect(git.stagePatch(request(d, [{ hunk: 0 }]))).rejects.toThrow(/changed since/);
    await git.unstagePatch(request(staged, [{ hunk: 0 }]));
    expect((await git.indexDiff("/README.md", "staged")).diff.hunks).toEqual([]);
    // A wrong side and unknown lines are refused.
    const fresh = await git.indexDiff("/README.md", "unstaged");
    await expect(git.stagePatch({ ...request(fresh, [{ hunk: 0 }]), side: "staged" })).rejects.toThrow();
    await expect(git.stagePatch(request(fresh, [{ hunk: 5 }]))).rejects.toThrow();
  });

  it("discards chosen lines from the file the index views see", async () => {
    const { git } = await repo();
    const d = await git.indexDiff("/README.md", "unstaged");
    await git.discardPatch(request(d, [{ hunk: 0 }]));
    expect((await git.indexDiff("/README.md", "unstaged")).diff.hunks).toEqual([]);
  });

  it("commits what is staged, whole request files only", async () => {
    const { b, git } = await repo();
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "https://example.test/staged" });
    await b.save();
    await expect(git.commitStaged({ message: "x" })).rejects.toThrow(/Nothing is staged/);
    await git.stage(["/health.yml"]);
    const done = await git.commitStaged({ message: "Staged only" });
    expect(done.summary).toBe("Staged only");
    expect((await git.status()).files).toEqual([]);
    const [check] = await git.verify([done.oid]);
    expect(check).toEqual({ oid: done.oid, state: "unsigned" });
  });

  it("answers config from the table tests fill", async () => {
    const { git } = await repo();
    expect(await git.config("commit.template")).toBeNull();
    MOCK_GIT_CONFIG.set("commit.template", "~/.gitmessage");
    expect(await git.config("commit.template")).toBe("~/.gitmessage");
    MOCK_GIT_CONFIG.clear();
  });
});
