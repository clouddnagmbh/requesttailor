// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's Git operations on Node's fs in a temporary folder; the `git` command line,
// when installed, checks that real Git sees the same repository.

import { execFileSync } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import git from "isomorphic-git";
import { afterEach, describe, expect, it } from "vitest";

import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";

const me = { name: "Test User", email: "test@example.com" };
const hasGit = (() => {
  try {
    execFileSync("git", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

function newRepo(files: Record<string, string> = {}): Repo & { write(p: string, t: string): void; read(p: string): string | null; rm(p: string): void } {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-git-"));
  dirs.push(dir);
  const write = (p: string, t: string) => {
    nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
    nodeFs.writeFileSync(join(dir, p), t);
  };
  for (const [p, t] of Object.entries(files)) write(p, t);
  return {
    fs: nodeFs as unknown as FsClient,
    dir,
    cache: {},
    write,
    read: (p) => (nodeFs.existsSync(join(dir, p)) ? nodeFs.readFileSync(join(dir, p), "utf8") : null),
    rm: (p) => nodeFs.rmSync(join(dir, p)),
  };
}

const cli = (r: Repo, ...args: string[]) =>
  execFileSync("git", args, { cwd: r.dir, encoding: "utf8", env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null" } });

const states = async (r: Repo) => (await repoOps.status(r)).files.map((f) => `${f.state} ${f.path}`);

/** A repository with one commit on `main` (a.yml, b.yml). */
async function committed() {
  const r = newRepo({ "a.yml": "a: 1\n", "b.yml": "b: 1\n" });
  await repoOps.init(r);
  await repoOps.commit(r, "first", ["a.yml", "b.yml"], me);
  return r;
}

/** Makes `refs/remotes/origin/main` the upstream of `main`, pointing at `oid`. */
async function setUpstream(r: Repo, oid: string) {
  await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: oid, force: true });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "remote.origin.url", value: "https://git.example/r.git" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
}

/** A commit on top of `parent` that changes `files`, made on a scratch branch (HEAD stays). */
async function commitElsewhere(r: Repo & { write(p: string, t: string): void }, files: Record<string, string>) {
  const back = (await git.currentBranch({ fs: r.fs, dir: r.dir }))!;
  await repoOps.createBranch(r, "scratch", true);
  for (const [p, t] of Object.entries(files)) r.write(p, t);
  const c = await repoOps.commit(r, "theirs", Object.keys(files), me);
  await repoOps.checkout(r, back);
  await git.deleteBranch({ fs: r.fs, dir: r.dir, ref: "scratch" });
  return c.oid;
}

describe("status", () => {
  it("reports no repository, then untracked, modified and deleted files", async () => {
    const r = newRepo({ "a.yml": "a: 1\n" });
    expect(await repoOps.status(r)).toEqual(repoOps.NOT_A_REPO);
    await repoOps.init(r);
    const fresh = await repoOps.status(r);
    expect(fresh).toMatchObject({ repo: true, branch: null, merging: false });
    expect(fresh.files).toEqual([{ path: "a.yml", state: "untracked" }]);
    await expect(repoOps.init(r)).rejects.toMatchObject({ kind: "git" });

    await repoOps.commit(r, "first", ["a.yml"], me);
    r.write("b.yml", "b\n");
    r.write("a.yml", "a: 2\n");
    expect(await states(r)).toEqual(["modified a.yml", "untracked b.yml"]);
    r.rm("a.yml");
    expect(await states(r)).toEqual(["deleted a.yml", "untracked b.yml"]);
    expect((await repoOps.status(r)).branch).toBe("main");
  });

  it("initializes a folder that has a stray .git/config", async () => {
    const r = newRepo({ ".git/config": "[core]\n", "a.yml": "a\n" });
    expect((await repoOps.status(r)).repo).toBe(false);
    await repoOps.init(r);
    await repoOps.commit(r, "first", ["a.yml"], me);
    expect(await repoOps.status(r)).toMatchObject({ repo: true, branch: "main", files: [] });
  });

  it("respects .gitignore", async () => {
    const r = newRepo({ ".gitignore": "secret.yml\n", "secret.yml": "x\n", "a.yml": "a\n" });
    await repoOps.init(r);
    expect(await states(r)).toEqual(["untracked .gitignore", "untracked a.yml"]);
  });

  it("counts commits ahead of and behind the upstream", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "c.yml": "c\n" });
    r.write("a.yml", "a: 2\n");
    await repoOps.commit(r, "second", ["a.yml"], me);
    r.write("a.yml", "a: 3\n");
    await repoOps.commit(r, "third", ["a.yml"], me);
    await setUpstream(r, theirs);
    expect(await repoOps.status(r)).toMatchObject({ upstream: "origin/main", ahead: 2, behind: 1 });
  });
});

describe("commit", () => {
  it("commits exactly the given paths and leaves everything else as it is", async () => {
    const r = await committed();
    r.write("a.yml", "a: 2\n");
    r.write("b.yml", "b: 2\n");
    r.write("new/c.yml", "c\n");
    // Staged elsewhere (e.g. the command line) but not selected: stays out of the commit.
    await git.add({ fs: r.fs, dir: r.dir, filepath: "b.yml" });
    const c = await repoOps.commit(r, "change a and add c\n\nbody", ["a.yml", "new/c.yml"], me);
    expect(c).toMatchObject({ summary: "change a and add c", author: "Test User" });
    expect(await states(r)).toEqual(["modified b.yml"]);
    const files = (await git.listFiles({ fs: r.fs, dir: r.dir, ref: "HEAD" })).sort();
    expect(files).toEqual(["a.yml", "b.yml", "new/c.yml"]);
    const blob = await git.readBlob({ fs: r.fs, dir: r.dir, oid: c.oid, filepath: "b.yml" });
    expect(new TextDecoder().decode(blob.blob)).toBe("b: 1\n");

    r.rm("a.yml");
    await repoOps.commit(r, "remove a", ["a.yml"], me);
    expect(await git.listFiles({ fs: r.fs, dir: r.dir, ref: "HEAD" })).not.toContain("a.yml");
    expect((await repoOps.log(r, 10)).map((x) => x.summary)).toEqual(["remove a", "change a and add c", "first"]);
    if (hasGit) expect(cli(r, "status", "--porcelain")).toBe("M  b.yml\n");
  });

  it("refuses empty messages, no paths and unchanged files", async () => {
    const r = await committed();
    await expect(repoOps.commit(r, " ", ["a.yml"], me)).rejects.toMatchObject({ kind: "git" });
    await expect(repoOps.commit(r, "x", [], me)).rejects.toMatchObject({ kind: "git" });
    await expect(repoOps.commit(r, "x", ["a.yml"], me)).rejects.toThrow(/no changes/);
  });
});

describe("branches", () => {
  it("creates, lists and switches branches; the files follow", async () => {
    const r = await committed();
    await repoOps.createBranch(r, "feature", true);
    r.write("a.yml", "a: feature\n");
    await repoOps.commit(r, "on feature", ["a.yml"], me);
    await repoOps.checkout(r, "main");
    expect(r.read("a.yml")).toBe("a: 1\n");
    expect(await repoOps.branches(r)).toEqual([
      { name: "feature", current: false, remote: false },
      { name: "main", current: true, remote: false },
    ]);
    await repoOps.checkout(r, "feature");
    expect(r.read("a.yml")).toBe("a: feature\n");
    await expect(repoOps.createBranch(r, "main", false)).rejects.toThrow(/already exists/);
  });

  it("checks out a remote branch as a local tracking branch", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "x.yml": "x\n" });
    await setUpstream(r, (await repoOps.log(r, 1))[0].oid);
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/topic", value: theirs });
    expect((await repoOps.branches(r)).filter((b) => b.remote).map((b) => b.name)).toEqual([
      "origin/main",
      "origin/topic",
    ]);
    await repoOps.checkout(r, "origin/topic");
    expect(await repoOps.status(r)).toMatchObject({ branch: "topic", upstream: "origin/topic", ahead: 0, behind: 0 });
    expect(r.read("x.yml")).toBe("x\n");
  });

  it("switches an empty repository to another unborn branch", async () => {
    const r = newRepo({ "a.yml": "a\n" });
    await repoOps.init(r);
    await repoOps.createBranch(r, "trunk", true);
    await repoOps.commit(r, "first", ["a.yml"], me);
    expect((await repoOps.status(r)).branch).toBe("trunk");
  });
});

describe("pull (merge of the fetched upstream)", () => {
  it("is up to date, then fast-forwards", async () => {
    const r = await committed();
    const first = (await repoOps.log(r, 1))[0].oid;
    await setUpstream(r, first);
    expect(await repoOps.mergeUpstream(r, me)).toEqual({ outcome: "up-to-date" });
    const theirs = await commitElsewhere(r, { "a.yml": "a: remote\n", "n.yml": "n\n" });
    await setUpstream(r, theirs);
    expect(await repoOps.mergeUpstream(r, me)).toEqual({ outcome: "fast-forward", commits: 1 });
    expect(r.read("a.yml")).toBe("a: remote\n");
    expect(r.read("n.yml")).toBe("n\n");
    expect(await states(r)).toEqual([]);
  });

  it("refuses when local changes are in the way", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "a.yml": "a: remote\n" });
    await setUpstream(r, theirs);
    r.write("a.yml", "a: local\n");
    await expect(repoOps.mergeUpstream(r, me)).rejects.toThrow(/Commit your changes to a\.yml/);
    expect(r.read("a.yml")).toBe("a: local\n");
  });

  it("merges diverged branches with a merge commit", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "b.yml": "b: remote\n" });
    r.write("a.yml", "a: local\n");
    await repoOps.commit(r, "local", ["a.yml"], me);
    await setUpstream(r, theirs);
    const result = await repoOps.mergeUpstream(r, me);
    expect(result.outcome).toBe("merged");
    expect(r.read("a.yml")).toBe("a: local\n");
    expect(r.read("b.yml")).toBe("b: remote\n");
    expect(await states(r)).toEqual([]);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: (result as { oid: string }).oid });
    expect(commit.parent).toHaveLength(2);
  });

  it("stops on conflicts; resolve and commit finish the merge", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "a.yml": "a: remote\n", "b.yml": "b: remote\n" });
    r.write("a.yml", "a: local\n");
    await repoOps.commit(r, "local", ["a.yml"], me);
    await setUpstream(r, theirs);

    expect(await repoOps.mergeUpstream(r, me)).toEqual({ outcome: "conflicts", paths: ["a.yml"] });
    const s = await repoOps.status(r);
    expect(s.merging).toBe(true);
    expect(s.files).toEqual([
      { path: "a.yml", state: "conflicted" },
      { path: "b.yml", state: "modified" },
    ]);
    if (hasGit) expect(cli(r, "status", "--porcelain")).toContain("UU a.yml");
    expect(await repoOps.conflict(r, "a.yml")).toEqual({
      path: "a.yml",
      base: "a: 1\n",
      ours: "a: local\n",
      theirs: "a: remote\n",
    });
    await expect(repoOps.checkout(r, "main")).rejects.toThrow(/Finish the merge/);
    await expect(repoOps.commit(r, "merge", ["a.yml", "b.yml"], me)).rejects.toThrow(/Resolve the conflict/);

    await repoOps.resolve(r, "a.yml", "theirs");
    expect(r.read("a.yml")).toBe("a: remote\n");
    expect(await states(r)).toEqual(["modified a.yml", "modified b.yml"]);
    expect((await repoOps.status(r)).merging).toBe(true);
    const merge = await repoOps.commit(r, "Merge origin/main into main", ["a.yml", "b.yml"], me);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: merge.oid });
    expect(commit.parent).toHaveLength(2);
    expect(await repoOps.status(r)).toMatchObject({ merging: false, files: [], ahead: 2, behind: 0 });
    if (hasGit) expect(cli(r, "status", "--porcelain")).toBe("");
  });

  it("resolves a conflict by keeping our side", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "a.yml": "a: remote\n" });
    r.write("a.yml", "a: local\n");
    await repoOps.commit(r, "local", ["a.yml"], me);
    await setUpstream(r, theirs);
    await repoOps.mergeUpstream(r, me);
    await repoOps.resolve(r, "a.yml", "ours");
    expect(r.read("a.yml")).toBe("a: local\n");
    expect(await states(r)).toEqual([]);
  });
});

describe("unmergedPaths", () => {
  it.skipIf(!hasGit)("reads conflicts from an index written by Git (versions 2 and 4)", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "a.yml": "a: remote\n", "deep/er/x.yml": "x\n" });
    r.write("a.yml", "a: local\n");
    r.write("deep/er/x.yml", "y\n");
    await repoOps.commit(r, "local", ["a.yml", "deep/er/x.yml"], me);
    try {
      cli(r, "-c", "user.name=T", "-c", "user.email=t@e", "merge", theirs);
    } catch {
      // Exits non-zero on conflicts.
    }
    const index = () => new Uint8Array(nodeFs.readFileSync(join(r.dir, ".git/index")));
    expect(repoOps.unmergedPaths(index())).toEqual(["a.yml", "deep/er/x.yml"]);
    cli(r, "update-index", "--index-version", "4");
    expect(repoOps.unmergedPaths(index())).toEqual(["a.yml", "deep/er/x.yml"]);
    expect(repoOps.unmergedPaths(new Uint8Array([1, 2, 3]))).toEqual([]);
  });
});

describe("cloneFolderName", () => {
  it("names the folder after the repository", () => {
    expect(repoOps.cloneFolderName("https://github.com/acme/api-tests.git")).toBe("api-tests");
    expect(repoOps.cloneFolderName("https://git.example/a/b/")).toBe("b");
    expect(repoOps.cloneFolderName("git@github.com:acme/x y.git")).toBe("x-y");
    expect(repoOps.cloneFolderName("https://git.example/")).toBe("git.example");
  });
});
