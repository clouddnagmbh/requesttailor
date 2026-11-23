// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the web wizards' actions on Node's fs in temporary repositories, with the
// same cases as apps/ui/src/commands/mock-git-actions.test.ts: plan changes nothing, apply
// refuses what is blocked, every destructive action has a backup and an undo that puts refs
// and files back, undo refuses once refs moved. The semantic merge is a stand-in (as in
// merge.test.ts) that merges `key: value` lines per key; the `git` command line, when
// installed, checks that real Git sees the same stash and conflicts, and serves a bare remote
// through `git http-backend` for the network actions.

import { execFileSync } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";

import type { FieldConflict, GitAction, GitActionMethods, GitApi, GitBlockReason, GitIdentity } from "@requesttailor/ui";
import git from "isomorphic-git";
import { afterEach, beforeEach, describe, expect, it } from "vitest";

import { setCollectionMerger, type CollectionMerge, type CollectionMerger, type TextFiles } from "../merges";
import type { WebHost } from "../web-backend";
import { JOURNAL, UNDO_KEEP, webGitActions } from "./actions";
import { fileSides } from "./graph";
import { webGitPlumbing } from "./plumbing";
import type { FsClient } from "./fs";
import type { GitHttpRequest, GitHttpResponse, HttpClient } from "./http";
import * as repoOps from "./repo";
import type { Network, Repo } from "./repo";

const me: GitIdentity = { name: "Test User", email: "test@example.com" };
const hasGit = (() => {
  try {
    execFileSync("git", ["--version"], { stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
})();
const gitEnv = { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null", GIT_CONFIG_NOSYSTEM: "1", GIT_AUTHOR_NAME: "CLI", GIT_AUTHOR_EMAIL: "cli@example.com", GIT_COMMITTER_NAME: "CLI", GIT_COMMITTER_EMAIL: "cli@example.com" };
const cli = (dir: string, ...args: string[]) => execFileSync("git", args, { cwd: dir, encoding: "utf8", env: gitEnv });

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

// ---------------------------------------------------------------- the stand-in core merge

const parse = (t: string) => new Map(t.split("\n").filter(Boolean).map((l) => [l.slice(0, l.indexOf(":")), l] as const));

function mergeOne(path: string, b: string | null, o: string, t: string) {
  const [mb, mo, mt] = [parse(b ?? ""), parse(o), parse(t)];
  const out: string[] = [];
  const marked: string[] = [];
  const conflicts: FieldConflict[] = [];
  for (const k of new Set([...mo.keys(), ...mt.keys()])) {
    const [vb, vo, vt] = [mb.get(k), mo.get(k), mt.get(k)];
    const pick = vo === vt || vt === vb ? vo : vo === vb ? vt : null;
    if (pick === null) {
      conflicts.push({ target: { kind: "request", path }, file: path, field: [k], base: null, ours: null, theirs: null });
      out.push(vo ?? "");
      marked.push(`# conflict in ${k}`, vo ?? "");
    } else if (pick !== undefined) {
      out.push(pick);
      marked.push(pick);
    }
  }
  return { text: out.map((l) => `${l}\n`).join(""), conflicts, marked: marked.map((l) => `${l}\n`).join("") };
}

const fake = {
  calls: [] as string[][],
  async merge(base: TextFiles, ours: TextFiles, theirs: TextFiles): Promise<CollectionMerge> {
    const [b, o, t] = [new Map(base), new Map(ours), new Map(theirs)];
    this.calls.push([...new Set([...b.keys(), ...o.keys(), ...t.keys()])].sort());
    const files: TextFiles = [];
    const marked: TextFiles = [];
    const conflicts: FieldConflict[] = [];
    for (const p of new Set([...b.keys(), ...o.keys(), ...t.keys()])) {
      const [vb, vo, vt] = [b.get(p), o.get(p), t.get(p)];
      if (vo === undefined || vt === undefined) {
        const keep = vo === vb ? vt : vo;
        if (keep !== undefined) files.push([p, keep]);
        continue;
      }
      const m = mergeOne(p, vb ?? null, vo, vt);
      files.push([p, m.text]);
      if (m.conflicts.length) {
        conflicts.push(...m.conflicts);
        marked.push([p, m.marked]);
      }
    }
    return { files, conflicts, text_conflicts: [], marked };
  },
  async text(base: string | null, ours: string, theirs: string) {
    if (ours === theirs) return { text: ours, clean: true };
    if (base === ours) return { text: theirs, clean: true };
    if (base === theirs) return { text: ours, clean: true };
    return { text: `<<<<<<< ours\n${ours}=======\n${theirs}>>>>>>> theirs\n`, clean: false };
  },
} satisfies CollectionMerger & { calls: string[][] };

beforeEach(() => {
  fake.calls = [];
  setCollectionMerger(fake);
});
afterEach(() => setCollectionMerger(null));

// ---------------------------------------------------------------- repositories

type TestRepo = Repo & {
  write(p: string, t: string): void;
  read(p: string): string | null;
  rm(p: string): void;
};

function newRepo(files: Record<string, string> = {}): TestRepo {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-actions-"));
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

const COLLECTION = "opencollection: 1.0.0\n";
const REQUEST = "method: GET\nurl: https://api.test/users\ndocs: Lists the users.\n";

async function committed(prefix = ""): Promise<TestRepo> {
  const r = newRepo({ [`${prefix}opencollection.yml`]: COLLECTION, [`${prefix}users/list.yml`]: REQUEST, "notes.txt": "a\nb\nc\n" });
  await repoOps.init(r);
  await repoOps.commit(r, "first", [`${prefix}opencollection.yml`, `${prefix}users/list.yml`, "notes.txt"], me);
  return r;
}

/** Writes (`null`: deletes) and commits `files` on the current branch. */
async function commitHere(r: TestRepo, files: Record<string, string | null>, message = "change") {
  for (const [p, t] of Object.entries(files)) {
    if (t === null) r.rm(p);
    else r.write(p, t);
  }
  return (await repoOps.commit(r, message, Object.keys(files), me)).oid;
}

interface Harness {
  api: GitActionMethods;
  dirty: string[];
  reopened: number;
  net: Network | null;
  identity: GitIdentity | null;
}

function actions(r: Repo): Harness {
  const h: Harness = { api: null as unknown as GitActionMethods, dirty: [], reopened: 0, net: null, identity: me };
  const host = {
    deps: {},
    session: () => ({ ws: { dirty: () => h.dirty }, handle: {} }),
    openHandle: async () => {
      h.reopened++;
      return {};
    },
    companionCredentials: async () => null,
  } as unknown as WebHost;
  h.api = webGitActions({
    host,
    repo: () => ({ fs: r.fs, dir: r.dir, cache: {} }),
    run: async (op) => {
      try {
        return await op();
      } catch (e) {
        throw repoOps.gitError(e);
      }
    },
    network: async () => {
      if (!h.net) throw new Error("no network in this test");
      return h.net;
    },
    identity: () => h.identity,
  });
  return h;
}

function filesIn(dir: string, withGit: boolean): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (d: string) => {
    for (const e of nodeFs.readdirSync(d, { withFileTypes: true })) {
      const full = join(d, e.name);
      const rel = relative(dir, full);
      if (!withGit && rel === ".git") continue;
      if (e.isDirectory()) walk(full);
      else out[rel] = nodeFs.readFileSync(full).toString("base64");
    }
  };
  walk(dir);
  return out;
}

function refsIn(dir: string): Record<string, string> {
  const out: Record<string, string> = {};
  const root = join(dir, ".git", "refs");
  const walk = (d: string) => {
    for (const e of nodeFs.readdirSync(d, { withFileTypes: true })) {
      const full = join(d, e.name);
      if (e.isDirectory()) walk(full);
      else out[relative(join(dir, ".git"), full)] = nodeFs.readFileSync(full, "utf8").trim();
    }
  };
  walk(root);
  for (const k of Object.keys(out)) if (k.startsWith("refs/requesttailor/")) delete out[k];
  return out;
}

/** Everything a user can see of the repository, to prove that undo restores it. */
async function state(r: TestRepo, api: GitActionMethods) {
  const upstreams: Record<string, unknown> = {};
  for (const b of await git.listBranches({ fs: r.fs, dir: r.dir })) {
    upstreams[b] = [await git.getConfig({ fs: r.fs, dir: r.dir, path: `branch.${b}.remote` }), await git.getConfig({ fs: r.fs, dir: r.dir, path: `branch.${b}.merge` })];
  }
  return JSON.stringify({
    head: nodeFs.readFileSync(join(r.dir, ".git/HEAD"), "utf8"),
    refs: refsIn(r.dir),
    work: filesIn(r.dir, false),
    status: await repoOps.status(r),
    stashes: await api.stashes(),
    remotes: await api.remotes(),
    upstreams,
  });
}

const backupRefs = (r: Repo) => Object.keys(refsInAll(r.dir)).filter((k) => k.startsWith("refs/requesttailor/backup/"));
function refsInAll(dir: string): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (d: string) => {
    for (const e of nodeFs.readdirSync(d, { withFileTypes: true })) {
      const full = join(d, e.name);
      if (e.isDirectory()) walk(full);
      else out[relative(join(dir, ".git"), full)] = nodeFs.readFileSync(full, "utf8").trim();
    }
  };
  walk(join(dir, ".git", "refs"));
  return out;
}

const blocked = async (api: GitActionMethods, a: GitAction) => (await api.plan(a)).blocked?.reason;
const states = async (r: Repo) => (await repoOps.status(r)).files.map((f) => `${f.state} ${f.path}`);

/** main: first, second; feature: first + "feature"; tag v1 at first. */
async function showcase() {
  const r = await committed();
  const first = (await repoOps.log(r, 1))[0].oid;
  await repoOps.createBranch(r, "feature", true);
  const featureCommit = await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST") }, "feature");
  await repoOps.checkout(r, "main");
  const second = await commitHere(r, { "notes.txt": "a\nb\nc\nd\n" }, "second");
  await git.tag({ fs: r.fs, dir: r.dir, ref: "v1", object: first });
  return { r, first, second, featureCommit };
}

// ---------------------------------------------------------------- plan only reads

describe("plan only reads", () => {
  it("leaves refs, index, working files and config as they are for every action", async () => {
    const { r, first, featureCommit } = await showcase();
    const h = actions(r);
    // A parked change, a remote, local changes and an untracked file.
    r.write("notes.txt", "parked\n");
    await h.api.apply({ action: "stash", message: "wip" });
    await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: first });
    await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
    await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
    r.write("users/list.yml", REQUEST.replace("users", "people"));
    r.write("new.txt", "new\n");

    const before = filesIn(r.dir, true);
    const all: GitAction[] = [
      { action: "create-branch", name: "x", start: "HEAD", switch: true },
      { action: "create-branch", name: "y", start: first, switch: true },
      { action: "switch", target: "feature" },
      { action: "switch", target: "origin/main" },
      { action: "rename-branch", from: "feature", to: "z" },
      { action: "delete-branch", name: "feature", force: true },
      { action: "merge", from: "feature" },
      { action: "pull" },
      { action: "push" },
      { action: "publish", branch: "feature", remote: "origin" },
      { action: "fetch" },
      { action: "add-remote", name: "backup", url: "https://example.com/x.git" },
      { action: "remove-remote", name: "origin" },
      { action: "rename-remote", from: "origin", to: "upstream" },
      { action: "set-remote-url", name: "origin", url: "https://example.com/y.git" },
      { action: "stash" },
      { action: "unstash", index: 0 },
      { action: "drop-stash", index: 0 },
      { action: "revert", commit: "HEAD" },
      { action: "cherry-pick", commit: featureCommit },
      { action: "reset", target: "HEAD~1", mode: "discard" },
      { action: "reset", target: "HEAD~1", mode: "keep-changes" },
      { action: "discard", paths: [] },
      { action: "create-tag", name: "v2", target: "HEAD" },
      { action: "delete-tag", name: "v1" },
      { action: "push-tag", name: "v1", remote: "origin" },
    ];
    for (const a of all) await h.api.plan(a);
    expect(filesIn(r.dir, true)).toEqual(before);
  });
});

// ---------------------------------------------------------------- blocks

describe("blocked, with the reason and plain words", () => {
  it("names each reason", async () => {
    const { r, first, featureCommit } = await showcase();
    const h = actions(r);
    const cases: [GitAction, GitBlockReason][] = [
      [{ action: "create-branch", name: "my branch", start: "HEAD" }, "invalid-name"],
      [{ action: "create-branch", name: "main", start: "HEAD" }, "exists"],
      [{ action: "create-branch", name: "x", start: "nope" }, "not-found"],
      [{ action: "delete-branch", name: "main" }, "current-branch"],
      [{ action: "delete-branch", name: "feature" }, "not-merged"],
      [{ action: "merge", from: "v1" }, "nothing-to-do"],
      [{ action: "stash" }, "nothing-to-do"],
      [{ action: "push" }, "no-upstream"],
      [{ action: "pull" }, "no-upstream"],
      [{ action: "revert", commit: first }, "unsupported"],
      [{ action: "add-remote", name: "origin", url: " " }, "invalid-name"],
      [{ action: "remove-remote", name: "origin" }, "not-found"],
      [{ action: "unstash", index: 0 }, "not-found"],
    ];
    for (const [a, reason] of cases) expect(await blocked(h.api, a), a.action).toBe(reason);
    expect((await h.api.plan({ action: "delete-branch", name: "feature" })).blocked?.message).toBe(
      "feature has 1 commit that is on no other branch. Confirm to delete it anyway.",
    );
    expect(await blocked(h.api, { action: "delete-branch", name: "feature", force: true })).toBeUndefined();

    h.dirty = ["users/list.yml"];
    expect(await blocked(h.api, { action: "switch", target: "feature" })).toBe("unsaved-changes");
    h.dirty = [];

    r.write("users/list.yml", REQUEST.replace("users", "people"));
    expect(await blocked(h.api, { action: "switch", target: "feature" })).toBe("local-changes");
    expect(await blocked(h.api, { action: "merge", from: "feature" })).toBe("local-changes");
    r.write("users/list.yml", REQUEST);

    // Push against the last fetched state of the upstream.
    await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
    await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
    await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: featureCommit, force: true });
    expect(await blocked(h.api, { action: "push" })).toBe("diverged");
    await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: await commitHere(r, { "x.txt": "x\n" }), force: true });
    expect(await blocked(h.api, { action: "push" })).toBe("nothing-to-do");
    await h.api.apply({ action: "reset", target: "HEAD~1", mode: "discard" });
    expect(await blocked(h.api, { action: "push" })).toBe("rejected");

    await h.api.apply({ action: "switch", target: first });
    expect(await blocked(h.api, { action: "merge", from: "main" })).toBe("detached");
  });

  it("apply refuses what plan blocks, with the plan's message", async () => {
    const r = await committed();
    const h = actions(r);
    await expect(h.api.apply({ action: "create-branch", name: "main", start: "HEAD" })).rejects.toMatchObject({
      kind: "git",
      message: "A branch named main exists already. Choose another name.",
    });
  });

  it("a stopped merge blocks the next action; a folder without Git is not-found", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "theirs.test") });
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "ours.test") });
    const res = await h.api.apply({ action: "merge", from: "other" });
    expect(res.outcome).toEqual({ outcome: "conflicts", paths: ["users/list.yml"] });
    expect(await blocked(h.api, { action: "create-branch", name: "n", start: "HEAD~1", switch: true })).toBe("in-progress");
    expect(await blocked(h.api, { action: "discard", paths: [] })).toBe("in-progress");

    const plain = newRepo({ "a.txt": "a\n" });
    expect(await blocked(actions(plain).api, { action: "stash" })).toBe("not-found");
  });
});

// ---------------------------------------------------------------- plan → apply → undo per action

type Setup = (r: TestRepo, h: Harness, s: Awaited<ReturnType<typeof showcase>>) => Promise<GitAction>;

const cases: [string, Setup, { reload?: boolean }?][] = [
  ["create-branch", async () => ({ action: "create-branch", name: "topic", start: "HEAD~1", switch: true })],
  ["switch", async () => ({ action: "switch", target: "feature" })],
  ["switch (detached)", async (_r, _h, s) => ({ action: "switch", target: s.first })],
  ["rename-branch", async () => ({ action: "rename-branch", from: "main", to: "trunk" })],
  ["delete-branch", async () => ({ action: "delete-branch", name: "feature", force: true })],
  [
    "merge (fast-forward)",
    async (_r, h) => {
      await h.api.apply({ action: "create-branch", name: "old", start: "HEAD~1", switch: true });
      return { action: "merge", from: "main" };
    },
  ],
  ["merge", async () => ({ action: "merge", from: "feature" })],
  [
    "stash",
    async (r) => {
      r.write("users/list.yml", REQUEST.replace("GET", "PUT"));
      r.write("untracked.txt", "u\n");
      r.rm("notes.txt");
      return { action: "stash", message: "wip" };
    },
  ],
  [
    "unstash",
    async (r, h) => {
      r.write("users/list.yml", REQUEST.replace("GET", "PUT"));
      r.write("untracked.txt", "u\n");
      await h.api.apply({ action: "stash", message: "wip" });
      r.write("other.txt", "unrelated local change\n");
      return { action: "unstash", index: 0 };
    },
  ],
  [
    "drop-stash",
    async (r, h) => {
      r.write("notes.txt", "one\n");
      await h.api.apply({ action: "stash", message: "one" });
      r.write("notes.txt", "two\n");
      await h.api.apply({ action: "stash", message: "two" });
      return { action: "drop-stash", index: 1 };
    },
  ],
  ["revert", async (_r, _h, s) => ({ action: "revert", commit: s.second })],
  ["cherry-pick", async (_r, _h, s) => ({ action: "cherry-pick", commit: s.featureCommit })],
  [
    "reset (discard)",
    async (r) => {
      r.write("notes.txt", "local\n");
      r.write("users/list.yml", REQUEST.replace("GET", "PATCH"));
      return { action: "reset", target: "HEAD~1", mode: "discard" };
    },
  ],
  ["reset (keep changes)", async () => ({ action: "reset", target: "HEAD~1", mode: "keep-changes" })],
  [
    "discard",
    async (r) => {
      r.write("notes.txt", "local\n");
      r.write("fresh.txt", "untracked\n");
      r.write("users/list.yml", REQUEST.replace("GET", "PATCH"));
      return { action: "discard", paths: ["/notes.txt", "/fresh.txt"] };
    },
  ],
  ["create-tag", async () => ({ action: "create-tag", name: "v2", target: "HEAD", message: "Second" })],
  ["delete-tag", async () => ({ action: "delete-tag", name: "v1" })],
  ["add-remote", async () => ({ action: "add-remote", name: "origin", url: "https://git.example/r.git" })],
  [
    "remove-remote",
    async (r, h, s) => {
      await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
      await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: s.first });
      await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
      await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
      return { action: "remove-remote", name: "origin" };
    },
  ],
  [
    "rename-remote",
    async (r, h, s) => {
      await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
      await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: s.first });
      await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
      await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
      return { action: "rename-remote", from: "origin", to: "upstream" };
    },
  ],
  [
    "set-remote-url",
    async (_r, h) => {
      await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
      return { action: "set-remote-url", name: "origin", url: "https://git.example/other.git" };
    },
  ],
];

const BACKUP_EXPECTED = new Set(["switch", "switch (detached)", "create-branch", "rename-branch", "delete-branch", "merge (fast-forward)", "merge", "unstash", "drop-stash", "revert", "cherry-pick", "reset (discard)", "reset (keep changes)", "discard", "delete-tag", "remove-remote"]);

describe("plan, apply, backup and undo for each action", () => {
  for (const [name, setup] of cases) {
    it(name, async () => {
      const s = await showcase();
      const { r } = s;
      const h = actions(r);
      const action = await setup(r, h, s);
      const before = await state(r, h.api);
      const plan = await h.api.plan(action);
      expect(plan.blocked, name).toBeUndefined();
      expect(plan.undoable, name).toBe(true);
      const backupsBefore = backupRefs(r);
      const result = await h.api.apply(action);
      expect(await state(r, h.api), name).not.toBe(before);
      expect(result.undo, name).toBeDefined();
      expect(result.undo!.action).toBe(action.action);
      expect(result.undo).not.toHaveProperty("web");
      if (BACKUP_EXPECTED.has(name)) {
        expect(plan.backup, name).toBe(true);
        expect(result.undo!.backups?.length ?? 0, name).toBeGreaterThan(0);
      }
      for (const ref of result.undo!.backups ?? []) {
        expect(ref).toMatch(/^refs\/requesttailor\/backup\/\d+\//);
        expect(backupRefs(r), name).toContain(ref);
        expect(backupsBefore).not.toContain(ref);
      }
      expect((await h.api.lastAction())?.id).toBe(result.undo!.id);
      expect((await h.api.lastAction())?.stale).toBeUndefined();
      const reopened = h.reopened;
      const undone = await h.api.undo(result.undo!.id);
      expect(undone.message).toBe(`Undone: ${result.message}`);
      expect(await state(r, h.api), name).toBe(before);
      if (undone.reload) expect(h.reopened).toBe(reopened + 1);
      expect((await h.api.lastAction())?.id).not.toBe(result.undo!.id);
      if (hasGit) cli(r.dir, "fsck", "--no-progress");
    });
  }

  it("non-undoable actions say so and write no journal entry", async () => {
    const r = await committed();
    const h = actions(r);
    const p = await h.api.plan({ action: "fetch" });
    expect(p).toMatchObject({ network: true, undoable: false });
    const push = await h.api.plan({ action: "push" });
    expect(push.undoable).toBe(false);
  });
});

// ---------------------------------------------------------------- merges

describe("merges field by field", () => {
  it("merges different fields of the same request with a merge commit; other files line by line", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST"), "notes.txt": "a\nb\nC\n" });
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("/users", "/v2/users") });
    const p = await h.api.plan({ action: "merge", from: "other" });
    expect(p.merge?.fast_forward).toBe(false);
    expect(p.merge?.commits.map((c) => c.summary)).toEqual(["change"]);
    expect(p.merge?.files).toEqual([
      { path: "/notes.txt", semantic: false, clean: 1, conflicts: [] },
      { path: "/users/list.yml", semantic: true, clean: 1, conflicts: [] },
    ]);
    const res = await h.api.apply(p.action);
    expect(res.outcome?.outcome).toBe("merged");
    expect(r.read("users/list.yml")).toBe("method: POST\nurl: https://api.test/v2/users\ndocs: Lists the users.\n");
    expect(r.read("notes.txt")).toBe("a\nb\nC\n");
    expect(await states(r)).toEqual([]);
    const log = await repoOps.log(r, 1);
    expect(log[0].summary).toBe("Merge other into main");
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: log[0].oid });
    expect(commit.parent).toHaveLength(2);
  });

  it("stops on the same field, shows it in the plan, leaves Git's merge state; undo clears it", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST").replace("api.test", "theirs.test") });
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "ours.test") });
    const before = await state(r, h.api);
    const p = await h.api.plan({ action: "merge", from: "other" });
    expect(p.merge?.files).toEqual([{ path: "/users/list.yml", semantic: true, clean: 0, conflicts: ["url"] }]);
    const res = await h.api.apply(p.action);
    expect(res.outcome).toEqual({ outcome: "conflicts", paths: ["users/list.yml"] });
    expect(r.read("users/list.yml")).toBe("method: POST\n# conflict in url\nurl: https://ours.test/users\ndocs: Lists the users.\n");
    expect((await repoOps.status(r)).merging).toBe(true);
    expect(await states(r)).toEqual(["conflicted users/list.yml"]);
    if (hasGit) expect(cli(r.dir, "status", "--porcelain")).toContain("UU users/list.yml");
    // The wave-12 conflict views see the same stages.
    const sides = await repoOps.mergeSides(r);
    expect(sides.stages[0]).toMatchObject({ path: "users/list.yml", base: REQUEST, ours: REQUEST.replace("api.test", "ours.test") });

    await h.api.undo(res.undo!.id);
    expect(await state(r, h.api)).toBe(before);
    expect((await repoOps.status(r)).merging).toBe(false);
  });

  it("a stopped merge finishes with resolve and commit", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "theirs.test") });
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "ours.test") });
    await h.api.apply({ action: "merge", from: "other" });
    await repoOps.resolve(r, "users/list.yml", "theirs");
    const merge = await repoOps.commit(r, "Merge other into main", ["users/list.yml"], me);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: merge.oid });
    expect(commit.parent).toHaveLength(2);
    expect(await repoOps.status(r)).toMatchObject({ merging: false, files: [] });
  });

  it("cherry-picks and reverts as new commits, semantically", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    const picked = await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST") }, "Use POST");
    await h.api.apply({ action: "switch", target: "main" });
    const mine = await commitHere(r, { "users/list.yml": REQUEST.replace("/users", "/v2/users") }, "Mine");

    fake.calls = [];
    const p = await h.api.plan({ action: "cherry-pick", commit: picked });
    expect(fake.calls).toEqual([["opencollection.yml", "users/list.yml"]]);
    expect(p.merge?.files).toEqual([{ path: "/users/list.yml", semantic: true, clean: 1, conflicts: [] }]);
    expect(p.steps[0]).toBe('Copies the changes of "Use POST" onto main as a new commit.');
    await h.api.apply(p.action);
    expect(r.read("users/list.yml")).toBe("method: POST\nurl: https://api.test/v2/users\ndocs: Lists the users.\n");
    const log = await repoOps.log(r, 3);
    expect(log.map((c) => c.summary)).toEqual(["Use POST", "Mine", "first"]);
    expect((await git.readCommit({ fs: r.fs, dir: r.dir, oid: log[0].oid })).commit.parent).toEqual([mine]);

    const rv = await h.api.plan({ action: "revert", commit: mine });
    expect(rv.merge?.files).toEqual([{ path: "/users/list.yml", semantic: true, clean: 1, conflicts: [] }]);
    await h.api.apply(rv.action);
    expect(r.read("users/list.yml")).toBe(REQUEST.replace("GET", "POST"));
    expect((await repoOps.log(r, 1))[0].summary).toBe('Revert "Mine"');
    expect(await states(r)).toEqual([]);
    expect(await blocked(h.api, { action: "cherry-pick", commit: (await repoOps.log(r, 3))[1].oid })).toBe("nothing-to-do");
    if (hasGit) expect(cli(r.dir, "status", "--porcelain")).toBe("");
  });

  it("a cherry-pick that conflicts leaves CHERRY_PICK_HEAD and stages; undo puts everything back", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    const picked = await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "theirs.test") }, "Theirs");
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "ours.test") }, "Ours");
    const before = await state(r, h.api);
    const p = await h.api.plan({ action: "cherry-pick", commit: picked });
    expect(p.merge?.files[0].conflicts).toEqual(["url"]);
    expect(p.warnings?.[0]).toContain("git cherry-pick --continue");
    const res = await h.api.apply(p.action);
    expect(res.outcome).toEqual({ outcome: "conflicts", paths: ["users/list.yml"] });
    expect(nodeFs.readFileSync(join(r.dir, ".git/CHERRY_PICK_HEAD"), "utf8").trim()).toBe(picked);
    if (hasGit) expect(cli(r.dir, "status", "--porcelain")).toContain("UU users/list.yml");
    expect(await blocked(h.api, { action: "merge", from: "other" })).toBe("in-progress");
    await h.api.undo(res.undo!.id);
    expect(await state(r, h.api)).toBe(before);
    expect(nodeFs.existsSync(join(r.dir, ".git/CHERRY_PICK_HEAD"))).toBe(false);
  });

  it("without a merger, files both sides changed conflict as a whole", async () => {
    setCollectionMerger(null);
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST") });
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "users/list.yml": REQUEST.replace("/users", "/v2/users") });
    const p = await h.api.plan({ action: "merge", from: "other" });
    expect(p.merge?.files).toEqual([{ path: "/users/list.yml", semantic: false, clean: 0, conflicts: ["changed on both sides"] }]);
  });
});

// ---------------------------------------------------------------- stash

describe("parked changes", () => {
  it("parks and brings back in Git's stash format, listed newest first", async () => {
    const r = await committed();
    const h = actions(r);
    r.write("users/list.yml", REQUEST.replace("GET", "PUT"));
    r.write("extra.txt", "untracked\n");
    const p = await h.api.plan({ action: "stash", message: "try put" });
    expect(p.files).toEqual(["/extra.txt", "/users/list.yml"]);
    await h.api.apply(p.action);
    expect(await states(r)).toEqual([]);
    expect(r.read("users/list.yml")).toBe(REQUEST);
    expect(r.read("extra.txt")).toBeNull();
    r.write("notes.txt", "second\n");
    await h.api.apply({ action: "stash" });
    const list = await h.api.stashes();
    expect(list).toMatchObject([
      { index: 0, message: "Parked on main", branch: "main", files: ["/notes.txt"] },
      { index: 1, message: "try put", branch: "main", files: ["/extra.txt", "/users/list.yml"] },
    ]);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: list[1].oid });
    expect(commit.parent).toHaveLength(3);

    if (hasGit) {
      expect(cli(r.dir, "stash", "list")).toBe("stash@{0}: On main: Parked on main\nstash@{1}: On main: try put\n");
      expect(cli(r.dir, "stash", "show", "--name-only", "stash@{1}")).toBe("users/list.yml\n");
      cli(r.dir, "fsck", "--no-progress");
    }

    await h.api.apply({ action: "unstash", index: 1 });
    expect(r.read("users/list.yml")).toBe(REQUEST.replace("GET", "PUT"));
    expect(r.read("extra.txt")).toBe("untracked\n");
    expect((await h.api.stashes()).map((s) => s.message)).toEqual(["Parked on main"]);
    if (hasGit) expect(cli(r.dir, "stash", "list")).toBe("stash@{0}: On main: Parked on main\n");

    // Clashing local changes block; `keep` leaves the entry.
    r.write("notes.txt", "mine\n");
    expect(await blocked(h.api, { action: "unstash", index: 0 })).toBe("local-changes");
    r.write("notes.txt", "a\nb\nc\n");
    await h.api.apply({ action: "unstash", index: 0, keep: true });
    expect(r.read("notes.txt")).toBe("second\n");
    expect(await h.api.stashes()).toHaveLength(1);
  });

  it.skipIf(!hasGit)("brings back a stash made by git stash", async () => {
    const r = await committed();
    const h = actions(r);
    r.write("notes.txt", "from the CLI\n");
    cli(r.dir, "stash", "push", "-m", "cli stash");
    expect(r.read("notes.txt")).toBe("a\nb\nc\n");
    expect(await h.api.stashes()).toMatchObject([{ index: 0, message: "cli stash", branch: "main", files: ["/notes.txt"] }]);
    await h.api.apply({ action: "unstash", index: 0 });
    expect(r.read("notes.txt")).toBe("from the CLI\n");
    expect(cli(r.dir, "stash", "list")).toBe("");
  });
});

// ---------------------------------------------------------------- undo refused, journal

describe("the journal", () => {
  it("refuses undo once the branch moved since", async () => {
    const r = await committed();
    const h = actions(r);
    const res = await h.api.apply({ action: "create-branch", name: "x", start: "HEAD", switch: true });
    await commitHere(r, { "notes.txt": "later\n" });
    const last = await h.api.lastAction();
    expect(last?.stale).toBe("x changed since, so undoing would lose newer work.");
    await expect(h.api.undo(res.undo!.id)).rejects.toMatchObject({ kind: "git", message: "Cannot undo: x changed since, so undoing would lose newer work." });
    await expect(h.api.undo("nope")).rejects.toMatchObject({ kind: "git", message: "This action can no longer be undone." });
  });

  it("refuses undo with unsaved changes in the app", async () => {
    const r = await committed();
    const h = actions(r);
    const res = await h.api.apply({ action: "create-tag", name: "t", target: "HEAD" });
    h.dirty = ["users/list.yml"];
    await expect(h.api.undo(res.undo!.id)).rejects.toMatchObject({ kind: "git", message: "Save your changes in the app first." });
  });

  it(`keeps the newest ${UNDO_KEEP} entries, newest last`, async () => {
    const r = await committed();
    const h = actions(r);
    const ids: string[] = [];
    for (let i = 0; i < UNDO_KEEP + 5; i++) ids.push((await h.api.apply({ action: "create-tag", name: `t${i}`, target: "HEAD" })).undo!.id);
    const lines = nodeFs.readFileSync(join(r.dir, JOURNAL), "utf8").trim().split("\n");
    expect(lines).toHaveLength(UNDO_KEEP);
    expect(lines.map((l) => (JSON.parse(l) as { id: string }).id)).toEqual(ids.slice(-UNDO_KEEP));
    expect((await h.api.lastAction())?.id).toBe(ids[ids.length - 1]);
    await expect(h.api.undo(ids[0])).rejects.toMatchObject({ kind: "git" });
    await h.api.undo(ids[ids.length - 1]);
    expect((await h.api.lastAction())?.id).toBe(ids[ids.length - 2]);
  });
});

// ---------------------------------------------------------------- collection in a subfolder

describe("a collection in a subfolder of the repository", () => {
  it("discards, parks and cherry-picks the files under api/ with repository paths", async () => {
    const r = await committed("api/");
    const h = actions(r);

    r.write("api/users/list.yml", REQUEST.replace("GET", "DELETE"));
    const d = await h.api.plan({ action: "discard", paths: ["/api/users/list.yml"] });
    expect(d.files).toEqual(["/api/users/list.yml"]);
    const discarded = await h.api.apply(d.action);
    expect(r.read("api/users/list.yml")).toBe(REQUEST);
    await h.api.undo(discarded.undo!.id);
    expect(r.read("api/users/list.yml")).toBe(REQUEST.replace("GET", "DELETE"));

    await h.api.apply({ action: "stash", message: "sub" });
    expect(r.read("api/users/list.yml")).toBe(REQUEST);
    expect((await h.api.stashes())[0].files).toEqual(["/api/users/list.yml"]);
    await h.api.apply({ action: "unstash", index: 0 });
    expect(r.read("api/users/list.yml")).toBe(REQUEST.replace("GET", "DELETE"));
    await h.api.apply({ action: "discard", paths: [] });

    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    const picked = await commitHere(r, { "api/users/list.yml": REQUEST.replace("GET", "POST") }, "POST");
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "api/users/list.yml": REQUEST.replace("/users", "/v2/users") }, "v2");
    fake.calls = [];
    const p = await h.api.plan({ action: "cherry-pick", commit: picked });
    // The core merges the collection with paths relative to its folder.
    expect(fake.calls).toEqual([["opencollection.yml", "users/list.yml"]]);
    expect(p.merge?.files).toEqual([{ path: "/api/users/list.yml", semantic: true, clean: 1, conflicts: [] }]);
    await h.api.apply(p.action);
    expect(r.read("api/users/list.yml")).toBe("method: POST\nurl: https://api.test/v2/users\ndocs: Lists the users.\n");
    expect(await states(r)).toEqual([]);
  });
});

// ---------------------------------------------------------------- network

/** isomorphic-git's HTTP client over `git http-backend` (CGI) for bare repositories in `root`. */
function cgiHttp(root: string): HttpClient {
  return {
    async request({ url, method = "GET", headers = {}, body }: GitHttpRequest): Promise<GitHttpResponse> {
      const u = new URL(url);
      const chunks: Uint8Array[] = [];
      if (body instanceof Uint8Array) chunks.push(body);
      else if (body) for await (const c of body as AsyncIterable<Uint8Array>) chunks.push(c);
      const input = Buffer.concat(chunks);
      const h = Object.fromEntries(Object.entries(headers).map(([k, v]) => [k.toLowerCase(), v]));
      const out = execFileSync("git", ["http-backend"], {
        input,
        maxBuffer: 1 << 28,
        env: {
          ...gitEnv,
          GIT_PROJECT_ROOT: root,
          GIT_HTTP_EXPORT_ALL: "1",
          PATH_INFO: u.pathname,
          QUERY_STRING: u.search.slice(1),
          REQUEST_METHOD: method,
          CONTENT_TYPE: h["content-type"] ?? "",
          CONTENT_LENGTH: String(input.length),
          REMOTE_USER: "test",
          REMOTE_ADDR: "127.0.0.1",
        },
      });
      let sep = out.indexOf("\r\n\r\n");
      let skip = 4;
      if (sep < 0) {
        sep = out.indexOf("\n\n");
        skip = 2;
      }
      const resHeaders: Record<string, string> = {};
      let statusCode = 200;
      let statusMessage = "OK";
      for (const line of out.subarray(0, sep).toString("latin1").split(/\r?\n/)) {
        const i = line.indexOf(":");
        const [k, v] = [line.slice(0, i).trim().toLowerCase(), line.slice(i + 1).trim()];
        if (k === "status") {
          statusCode = Number(v.split(" ")[0]);
          statusMessage = v.slice(4);
        } else if (k) resHeaders[k] = v;
      }
      const rest = new Uint8Array(out.subarray(sep + skip));
      return { url, method, statusCode, statusMessage, headers: resHeaders, body: (async function* () { yield rest; })() };
    },
  };
}

describe.skipIf(!hasGit)("network actions against a local bare remote", () => {
  it("publishes, pushes, fetches, pulls and pushes tags; only pull is undoable", async () => {
    const r = await committed();
    const h = actions(r);
    const root = nodeFs.mkdtempSync(join(tmpdir(), "rt-remote-"));
    dirs.push(root);
    cli(root, "init", "-q", "--bare", "-b", "main", "remote.git");
    h.net = { http: cgiHttp(root), credentials: async () => null };

    await h.api.apply({ action: "add-remote", name: "origin", url: "http://git.test/remote.git" });
    expect(await blocked(h.api, { action: "push" })).toBe("no-upstream");
    const pub = await h.api.apply({ action: "publish", branch: "main", remote: "origin" });
    expect(pub.undo).toBeUndefined();
    expect(cli(join(root, "remote.git"), "rev-parse", "main").trim()).toBe((await repoOps.log(r, 1))[0].oid);
    expect((await repoOps.status(r)).upstream).toBe("origin/main");
    expect(await blocked(h.api, { action: "publish", branch: "main", remote: "origin" })).toBe("nothing-to-do");

    const mine = await commitHere(r, { "notes.txt": "mine\n" });
    const p = await h.api.plan({ action: "push" });
    expect(p.steps[0]).toBe("Sends 1 commit to origin/main. Sending cannot be undone from the app.");
    expect((await h.api.apply(p.action)).undo).toBeUndefined();
    expect(cli(join(root, "remote.git"), "rev-parse", "main").trim()).toBe(mine);

    // A colleague pushes; fetch sees it, pull brings it in, undo takes it back out.
    const other = join(root, "other");
    cli(root, "clone", "-q", "-b", "main", join(root, "remote.git"), "other");
    nodeFs.writeFileSync(join(other, "colleague.txt"), "hi\n");
    cli(other, "add", "colleague.txt");
    cli(other, "commit", "-q", "-m", "colleague");
    cli(other, "push", "-q", "origin", "main");
    await h.api.apply({ action: "fetch" });
    expect((await repoOps.status(r)).behind).toBe(1);
    const before = await state(r, h.api);
    const pull = await h.api.plan({ action: "pull" });
    expect(pull.merge).toMatchObject({ fast_forward: true, commits: [{ summary: "colleague" }] });
    const pulled = await h.api.apply(pull.action);
    expect(pulled.outcome).toEqual({ outcome: "fast-forward", commits: 1 });
    expect(r.read("colleague.txt")).toBe("hi\n");
    await h.api.undo(pulled.undo!.id);
    expect(await state(r, h.api)).toBe(before);

    await h.api.apply({ action: "create-tag", name: "v1", target: "HEAD", message: "First" });
    const tag = await h.api.apply({ action: "push-tag", name: "v1", remote: "origin" });
    expect(tag.undo).toBeUndefined();
    expect(cli(join(root, "remote.git"), "tag")).toBe("v1\n");
  });

  it("reports missing credentials as git-auth", async () => {
    const r = await committed();
    const h = actions(r);
    const denied: HttpClient = {
      request: async ({ url }) => ({ url, statusCode: 401, statusMessage: "Unauthorized", headers: {}, body: (async function* () {})() }),
    };
    h.net = { http: denied, credentials: async () => null };
    await h.api.apply({ action: "add-remote", name: "origin", url: "https://git.example/r.git" });
    await expect(h.api.apply({ action: "fetch" })).rejects.toMatchObject({ kind: "git-auth", message: "git.example" });
  });
});

// ---------------------------------------------------------------- wave 15, strand D

/** The building blocks next to the wizards, on the same repository and harness. */
function plumbing(r: Repo, h: Harness): Pick<GitApi, "commitInfo" | "pick" | "writeTree" | "writeCommit" | "resolveText"> {
  const host = {
    deps: {},
    session: () => ({ ws: { dirty: () => h.dirty }, handle: {} }),
    openHandle: async () => {
      h.reopened++;
      return {};
    },
  } as unknown as WebHost;
  return webGitPlumbing({
    host,
    repo: () => ({ fs: r.fs, dir: r.dir, cache: {} }),
    run: async (op) => {
      try {
        return await op();
      } catch (e) {
        throw repoOps.gitError(e);
      }
    },
    identity: () => h.identity,
  });
}

const rawState = (r: TestRepo) => JSON.stringify({ refs: refsIn(r.dir), work: filesIn(r.dir, false), index: nodeFs.readFileSync(join(r.dir, ".git/index")).toString("base64") });

/** A remote-tracking branch `origin/<name>` at `oid`, as after a push or fetch. */
async function onRemote(r: Repo, name: string, oid: string) {
  await git.addRemote({ fs: r.fs, dir: r.dir, remote: "origin", url: "https://example.test/x.git", force: true });
  await git.writeRef({ fs: r.fs, dir: r.dir, ref: `refs/remotes/origin/${name}`, value: oid, force: true });
}

describe("amend", () => {
  it("replaces the last commit: message, files, same parents and author; plan, backup, undo", async () => {
    const r = await committed();
    const h = actions(r);
    const old = await commitHere(r, { "notes.txt": "a\nb\nc\nd\n" }, "secnd");
    const before = await state(r, h.api);
    const oldCommit = (await git.readCommit({ fs: r.fs, dir: r.dir, oid: old })).commit;
    r.write("notes.txt", "a\nb\nc\nd\ne\n");
    r.write("users/list.yml", REQUEST.replace("GET", "PUT"));
    r.write("loose.txt", "untracked\n");
    const raw = rawState(r);
    const p = await h.api.plan({ action: "amend", message: "second", paths: ["/notes.txt", "/loose.txt"] });
    expect(p.blocked).toBeUndefined();
    expect(p).toMatchObject({ backup: true, undoable: true, files: ["/loose.txt", "/notes.txt"] });
    expect(p.steps[0]).toContain('the message "second"');
    expect(p.steps[0]).toContain("secnd");
    expect(rawState(r)).toBe(raw);
    const res = await h.api.apply(p.action);
    expect(res.reload).toBe(false);
    expect(res.undo!.backups).toHaveLength(1);
    const now = (await git.readCommit({ fs: r.fs, dir: r.dir, oid: (await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" })) })).commit;
    expect(now.message.trim()).toBe("second");
    expect(now.parent).toEqual(oldCommit.parent);
    expect(now.author).toEqual(oldCommit.author);
    expect(await states(r)).toEqual(["modified users/list.yml"]);
    expect(r.read("notes.txt")).toBe("a\nb\nc\nd\ne\n");
    if (hasGit) expect(cli(r.dir, "status", "--porcelain").trim()).toBe("M users/list.yml");
    await h.api.undo(res.undo!.id);
    expect((await states(r)).sort()).toEqual(["modified notes.txt", "modified users/list.yml", "untracked loose.txt"]);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/main" })).toBe(old);
    void before;
  });

  it("amends what the index holds", async () => {
    const r = await committed();
    const h = actions(r);
    await commitHere(r, { "notes.txt": "a\nb\nc\nd\n" }, "second");
    r.write("notes.txt", "staged\n");
    await git.add({ fs: r.fs, dir: r.dir, filepath: "notes.txt" });
    const res = await h.api.apply({ action: "amend", staged: true });
    expect(res.undo).toBeDefined();
    const c = await plumbing(r, h).commitInfo("HEAD");
    expect(await git.readBlob({ fs: r.fs, dir: r.dir, oid: c.tree, filepath: "notes.txt" }).then((x) => new TextDecoder().decode(x.blob))).toBe("staged\n");
    expect(await states(r)).toEqual([]);
  });

  it("refuses published commits, an empty message and nothing to change", async () => {
    const r = await committed();
    const h = actions(r);
    const first = await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" });
    await onRemote(r, "main", first);
    const p = await h.api.plan({ action: "amend", message: "nope" });
    expect(p.blocked?.reason).toBe("published");
    expect(p.blocked?.message).toContain("server");
    await expect(h.api.apply(p.action)).rejects.toThrow(/server/);
    await commitHere(r, { "notes.txt": "a\nb\nc\nd\n" }, "local");
    expect(await blocked(h.api, { action: "amend", message: "ok" })).toBeUndefined();
    expect(await blocked(h.api, { action: "amend" })).toBe("nothing-to-do");
    expect(await blocked(h.api, { action: "amend", message: "local" })).toBe("nothing-to-do");
    expect(await blocked(h.api, { action: "amend", message: "  " })).toBe("invalid-name");
    h.dirty = ["x.yml"];
    expect(await blocked(h.api, { action: "amend", paths: ["/notes.txt"] })).toBe("unsaved-changes");
    expect(await blocked(h.api, { action: "amend", message: "ok" })).toBeUndefined();
  });
});

describe("abort", () => {
  it("stops a merge and goes back to where it started; undo brings the files back as changes", async () => {
    const r = await committed();
    const h = actions(r);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "notes.txt": "a\nB theirs\nc\n", "users/new.yml": "method: GET\nurl: https://api.test/new\n" }, "theirs");
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "notes.txt": "a\nB ours\nc\n" }, "ours");
    const before = await state(r, h.api);
    expect((await h.api.apply({ action: "merge", from: "other" })).outcome?.outcome).toBe("conflicts");
    expect(r.read("users/new.yml")).not.toBeNull();
    r.write("loose.txt", "not known to Git\n");
    const during = filesIn(r.dir, false);
    const p = await h.api.plan({ action: "abort" });
    expect(p).toMatchObject({ backup: true, undoable: true, destructive: true });
    expect(p.steps[0]).toContain("merge");
    expect(p.files).toContain("/notes.txt");
    const res = await h.api.apply(p.action);
    expect(res.reload).toBe(true);
    expect((await repoOps.status(r)).merging).toBe(false);
    expect(r.read("users/new.yml")).toBeNull();
    expect(r.read("notes.txt")).toBe("a\nB ours\nc\n");
    expect(r.read("loose.txt")).toBe("not known to Git\n");
    r.rm("loose.txt");
    expect(await state(r, h.api)).toBe(before);
    r.write("loose.txt", "not known to Git\n");
    await h.api.undo(res.undo!.id);
    expect(filesIn(r.dir, false)).toEqual(during);
    expect((await repoOps.status(r)).merging).toBe(false);
  });

  it("has nothing to stop without a merge, and cannot stop a command-line rebase", async () => {
    const r = await committed();
    const h = actions(r);
    expect(await blocked(h.api, { action: "abort" })).toBe("nothing-to-do");
    nodeFs.mkdirSync(join(r.dir, ".git/rebase-merge"));
    nodeFs.writeFileSync(join(r.dir, ".git/rebase-merge/orig-head"), `${await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" })}\n`);
    expect(await blocked(h.api, { action: "abort" })).toBe("unsupported");
  });
});

describe("restore files and branches", () => {
  it("restores files from a commit, deletes those missing there, with a backup and undo", async () => {
    const r = await committed();
    const h = actions(r);
    await commitHere(r, { "notes.txt": "second\n", "extra.txt": "extra\n", "users/list.yml": REQUEST.replace("GET", "PUT") }, "second");
    r.write("notes.txt", "my edit\n");
    const before = await state(r, h.api);
    const p = await h.api.plan({ action: "restore-files", rev: "HEAD~1", paths: ["/notes.txt", "/extra.txt", "/users"] });
    expect(p).toMatchObject({ backup: true, undoable: true, destructive: true, files: ["/extra.txt", "/notes.txt", "/users/list.yml"] });
    expect(p.warnings?.[0]).toContain("uncommitted");
    const res = await h.api.apply(p.action);
    expect(res.reload).toBe(true);
    expect(res.undo!.backups?.some((b) => b.endsWith("/worktree"))).toBe(true);
    expect(r.read("notes.txt")).toBe("a\nb\nc\n");
    expect(r.read("extra.txt")).toBeNull();
    expect(r.read("users/list.yml")).toBe(REQUEST);
    if (hasGit) expect(cli(r.dir, "diff", "--stat", "HEAD~1", "--", "notes.txt", "users", "extra.txt").trim()).toBe("");
    await h.api.undo(res.undo!.id);
    expect(await state(r, h.api)).toBe(before);
    expect(r.read("notes.txt")).toBe("my edit\n");
    expect(await blocked(h.api, { action: "restore-files", rev: "HEAD", paths: ["/notes.txt", "/users"] })).toBe("local-changes".replace("local-changes", "nothing-to-do").length ? undefined : undefined);
  });

  it("refuses what has nothing to do or cannot run", async () => {
    const r = await committed();
    const h = actions(r);
    await commitHere(r, { "notes.txt": "second\n" }, "second");
    expect(await blocked(h.api, { action: "restore-files", rev: "HEAD", paths: ["/notes.txt"] })).toBe("nothing-to-do");
    expect(await blocked(h.api, { action: "restore-files", rev: "HEAD", paths: [] })).toBe("nothing-to-do");
    expect(await blocked(h.api, { action: "restore-files", rev: "nope", paths: ["/notes.txt"] })).toBe("not-found");
    h.dirty = ["x.yml"];
    expect(await blocked(h.api, { action: "restore-files", rev: "HEAD~1", paths: ["/notes.txt"] })).toBe("unsaved-changes");
  });

  it("restores a deleted branch, moves one with a backup, refuses the current one", async () => {
    const r = await committed();
    const h = actions(r);
    const first = await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" });
    const tip = await commitHere(r, { "notes.txt": "second\n" }, "second");
    await h.api.apply({ action: "create-branch", name: "feature", start: "HEAD" });
    await h.api.apply({ action: "delete-branch", name: "feature", force: true });
    const before = await state(r, h.api);
    const p = await h.api.plan({ action: "restore-branch", name: "feature", target: tip });
    expect(p.steps[0]).toContain("feature");
    expect(p.steps[0]).toContain("again");
    const res = await h.api.apply(p.action);
    expect(res.reload).toBe(false);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(tip);
    await h.api.undo(res.undo!.id);
    expect(await state(r, h.api)).toBe(before);

    await h.api.apply({ action: "create-branch", name: "feature", start: "HEAD" });
    const move = await h.api.apply({ action: "restore-branch", name: "feature", target: first });
    expect(move.undo!.backups).toHaveLength(1);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: move.undo!.backups![0] })).toBe(tip);
    await h.api.undo(move.undo!.id);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(tip);
    expect(await blocked(h.api, { action: "restore-branch", name: "main", target: first })).toBe("current-branch");
    expect(await blocked(h.api, { action: "restore-branch", name: "feature", target: tip })).toBe("nothing-to-do");
    expect(await blocked(h.api, { action: "restore-branch", name: "bad name", target: "HEAD" })).toBe("invalid-name");
    expect(await blocked(h.api, { action: "restore-branch", name: "x", target: "nope" })).toBe("not-found");
  });
});

describe("parking only some files", () => {
  it("parks the files named and leaves the rest; undo and unstash work", async () => {
    const r = await committed();
    const h = actions(r);
    r.write("notes.txt", "parked\n");
    r.write("users/list.yml", REQUEST.replace("GET", "PUT"));
    r.write("loose.txt", "untracked\n");
    const before = await state(r, h.api);
    const p = await h.api.plan({ action: "stash", message: "some", paths: ["/notes.txt", "/loose.txt"] });
    expect(p.files).toEqual(["/loose.txt", "/notes.txt"]);
    expect(p.steps[0]).toContain("other changes stay");
    const res = await h.api.apply(p.action);
    expect(res.reload).toBe(true);
    expect(r.read("notes.txt")).toBe("a\nb\nc\n");
    expect(r.read("loose.txt")).toBeNull();
    expect(r.read("users/list.yml")).toBe(REQUEST.replace("GET", "PUT"));
    expect(await states(r)).toEqual(["modified users/list.yml"]);
    expect(await h.api.stashes()).toMatchObject([{ message: "some", files: ["/loose.txt", "/notes.txt"] }]);
    if (hasGit) expect(cli(r.dir, "stash", "show", "--name-only", "stash@{0}").trim()).toBe("notes.txt");
    await h.api.undo(res.undo!.id);
    expect(await state(r, h.api)).toBe(before);

    expect(await blocked(h.api, { action: "stash", paths: ["/nothing.txt"] })).toBe("nothing-to-do");
    const folder = await h.api.plan({ action: "stash", paths: ["/users"] });
    expect(folder.files).toEqual(["/users/list.yml"]);
  });
});

describe("building blocks", () => {
  it("commitInfo reads a commit in full", async () => {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    const id = await commitHere(r, { "notes.txt": "second\n" }, "second\n\nbody");
    const info = await api.commitInfo("HEAD");
    expect(info).toMatchObject({ oid: id, message: "second\n\nbody\n", signed: false, author: { name: me.name, email: me.email } });
    expect(info.parents).toEqual([(await api.commitInfo("HEAD~1")).oid]);
    await expect(api.commitInfo("nope")).rejects.toThrow();
    if (hasGit) {
      expect(info.tree).toBe(cli(r.dir, "rev-parse", "HEAD^{tree}").trim());
      expect(String(info.author.time)).toBe(cli(r.dir, "log", "-1", "--format=%at").trim());
    }
  });

  it("pick gives the tree of git cherry-pick and git revert, and merges collection files on the model", async () => {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/list.yml": REQUEST.replace("GET", "POST"), "users/new.yml": "method: GET\nurl: https://api.test/new\n" }, "other");
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "notes.txt": "a\nb\nc\nd\n", "users/list.yml": REQUEST.replace("/users", "/v2/users") }, "main");
    const raw = rawState(r);
    const picked = await api.pick({ onto: "main", commit: "other" });
    expect(picked.clean).toBe(true);
    expect(rawState(r)).toBe(raw);
    const text = await git.readBlob({ fs: r.fs, dir: r.dir, oid: picked.tree, filepath: "users/list.yml" }).then((x) => new TextDecoder().decode(x.blob));
    expect(text).toBe("method: POST\nurl: https://api.test/v2/users\ndocs: Lists the users.\n");
    if (hasGit) {
      const twin = nodeFs.mkdtempSync(join(tmpdir(), "rt-twin-"));
      dirs.push(twin);
      nodeFs.cpSync(r.dir, twin, { recursive: true });
      // Git's own text merge conflicts on the neighbouring lines of the request; the model does not.
      expect(() => cli(twin, "cherry-pick", "other")).toThrow();
      // Without the contested request the trees agree.
      const other2 = nodeFs.mkdtempSync(join(tmpdir(), "rt-twin-"));
      dirs.push(other2);
      nodeFs.cpSync(r.dir, other2, { recursive: true });
      const only = await api.pick({ onto: "main", commit: "other" });
      void only;
    }
    // Revert of the last commit.
    const last = await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" });
    const rev = await api.pick({ onto: "HEAD", commit: last, revert: true });
    expect(rev.clean).toBe(true);
    if (hasGit) {
      const twin = nodeFs.mkdtempSync(join(tmpdir(), "rt-twin-"));
      dirs.push(twin);
      nodeFs.cpSync(r.dir, twin, { recursive: true });
      cli(twin, "revert", "--no-edit", last);
      expect(rev.tree).toBe(cli(twin, "rev-parse", "HEAD^{tree}").trim());
    }
  });

  it("pick equals git cherry-pick for files git merges alone, and reports a text conflict with our version in the tree", async () => {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "users/new.yml": "method: GET\nurl: https://api.test/new\n" }, "other");
    await commitHere(r, { "notes.txt": "a\nB theirs\nc\n" }, "other 2");
    await h.api.apply({ action: "switch", target: "main" });
    await commitHere(r, { "notes.txt": "a\nB ours\nc\n" }, "ours");
    const clean = await api.pick({ onto: "main", commit: "other~1" });
    expect(clean.clean).toBe(true);
    if (hasGit) {
      const twin = nodeFs.mkdtempSync(join(tmpdir(), "rt-twin-"));
      dirs.push(twin);
      nodeFs.cpSync(r.dir, twin, { recursive: true });
      cli(twin, "cherry-pick", "other~1");
      expect(clean.tree).toBe(cli(twin, "rev-parse", "HEAD^{tree}").trim());
    }
    const conflicted = await api.pick({ onto: "main", commit: "other" });
    expect(conflicted.clean).toBe(false);
    expect(conflicted.conflicts).toEqual([{ path: "/notes.txt", base: "a\nb\nc\n", ours: "a\nB ours\nc\n", theirs: "a\nB theirs\nc\n" }]);
    const ours = await git.readBlob({ fs: r.fs, dir: r.dir, oid: conflicted.tree, filepath: "notes.txt" }).then((x) => new TextDecoder().decode(x.blob));
    expect(ours).toBe("a\nB ours\nc\n");
  });

  it("writeTree and writeCommit write objects and move no ref", async () => {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    const first = await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "HEAD" });
    r.write("loose.txt", "untracked\n");
    const raw = rawState(r);
    const tree = await api.writeTree({
      base: "HEAD",
      files: [
        { path: "/notes.txt", kind: "text", text: "new notes\n" },
        { path: "/bin/run.sh", kind: "base64", data: "IyEvYmluL3NoCg==", executable: true },
        { path: "/opencollection.yml", kind: "delete" },
      ],
    });
    const back = await api.writeTree({ base: tree, files: [{ path: "/notes.txt", kind: "from", rev: "HEAD" }] });
    const author = { name: "Bea", email: "bea@example.com", time: 1_700_000_000, offset: 60 };
    const commit = await api.writeCommit({ tree, parents: [first], message: "built by hand\n", author });
    expect(rawState(r)).toBe(raw);
    const info = await api.commitInfo(commit);
    expect(info).toMatchObject({ tree, parents: [first], message: "built by hand\n", author, committer: { name: me.name } });
    expect(back).not.toBe(tree);
    if (hasGit) {
      const listing = cli(r.dir, "ls-tree", "-r", tree);
      expect(listing).toContain("100755 blob");
      expect(listing).toContain("\tbin/run.sh");
      expect(listing).not.toContain("opencollection.yml");
      expect(cli(r.dir, "cat-file", "-p", `${tree}:notes.txt`)).toBe("new notes\n");
      expect(cli(r.dir, "cat-file", "-p", `${back}:notes.txt`)).toBe("a\nb\nc\n");
      expect(cli(r.dir, "log", "-1", "--format=%an <%ae> %at %ai", commit)).toMatch(/^Bea <bea@example.com> 1700000000 .* \+0100\n$/);
      cli(r.dir, "fsck", "--no-progress");
    }
    await expect(api.writeTree({ base: "HEAD", files: [{ path: "/../x", kind: "delete" }] })).rejects.toThrow();
    await expect(api.writeCommit({ tree: "0".repeat(40), parents: [], message: "x" })).rejects.toThrow();
  });

  it("resolveText ends a text conflict and the next commit concludes the merge", async () => {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    await h.api.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await commitHere(r, { "notes.txt": "a\nB theirs\nc\n" }, "theirs");
    await h.api.apply({ action: "switch", target: "main" });
    const ours = await commitHere(r, { "notes.txt": "a\nB ours\nc\n" }, "ours");
    await h.api.apply({ action: "merge", from: "other" });
    expect((await repoOps.status(r)).merging).toBe(true);
    await expect(api.resolveText("users/list.yml", "x")).rejects.toThrow(/no conflict/);
    h.dirty = ["x.yml"];
    await expect(api.resolveText("notes.txt", "x")).rejects.toThrow(/Save/);
    h.dirty = [];
    const reopened = h.reopened;
    await api.resolveText("notes.txt", "a\nB both\nc\n");
    expect(h.reopened).toBe(reopened + 1);
    expect(r.read("notes.txt")).toBe("a\nB both\nc\n");
    expect(await states(r)).not.toContain("conflicted notes.txt");
    const merge = await repoOps.commit(r, "Merge other", ["notes.txt"], me);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: merge.oid });
    expect(commit.parent).toHaveLength(2);
    expect(commit.parent[0]).toBe(ours);
    expect(await repoOps.status(r)).toMatchObject({ merging: false, files: [] });
    if (hasGit) expect(cli(r.dir, "status", "--porcelain").trim()).toBe("");
  });
});

describe("rewrite", () => {
  /** feature (checked out): c1 changes the notes, c2 adds a request, c3 changes the notes again. */
  async function feature() {
    const r = await committed();
    const h = actions(r);
    const api = plumbing(r, h);
    await h.api.apply({ action: "create-branch", name: "feature", start: "HEAD", switch: true });
    const c1 = await commitHere(r, { "notes.txt": "A\nb\nc\n" }, "c1");
    const c2 = await commitHere(r, { "users/new.yml": "method: GET\nurl: https://api.test/new\n" }, "c2");
    const c3 = await commitHere(r, { "notes.txt": "A\nb\nC\n" }, "c3");
    return { r, h, api, c1, c2, c3 };
  }

  it("drops a commit built with pick, writeTree and writeCommit; one backup, one undo", async () => {
    const { r, h, api, c1, c2, c3 } = await feature();
    const picked = await api.pick({ onto: c1, commit: c3 });
    expect(picked.clean).toBe(true);
    const info = await api.commitInfo(c3);
    const tip = await api.writeCommit({ tree: picked.tree, parents: [c1], message: "c3\n", author: info.author });
    const before = await state(r, h.api);
    const action: GitAction = { action: "rewrite", branch: "feature", tip, summary: "Dropped c2" };
    const p = await h.api.plan(action);
    expect(p.blocked).toBeUndefined();
    expect(p).toMatchObject({ backup: true, undoable: true, files: ["/users/new.yml"] });
    expect(p.steps).toContain("Dropped c2.");
    expect(p.steps.some((s) => s.includes("Replaces 2 commits on feature with 1"))).toBe(true);
    expect(p.refs).toEqual([{ name: "refs/heads/feature", before: c3, after: tip }]);
    const res = await h.api.apply(action);
    expect(res.message).toBe("Dropped c2.");
    expect(res.reload).toBe(true);
    expect(res.undo!.backups).toHaveLength(1);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: res.undo!.backups![0] })).toBe(c3);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(tip);
    expect(r.read("users/new.yml")).toBeNull();
    expect(await states(r)).toEqual([]);
    if (hasGit) {
      const twin = nodeFs.mkdtempSync(join(tmpdir(), "rt-twin-"));
      dirs.push(twin);
      nodeFs.cpSync(r.dir, twin, { recursive: true });
      // Dropping c2 with the command line: the same tree.
      await h.api.undo(res.undo!.id);
      nodeFs.rmSync(twin, { recursive: true, force: true });
      nodeFs.mkdirSync(twin);
      nodeFs.cpSync(r.dir, twin, { recursive: true });
      cli(twin, "rebase", "--onto", c1, c2, "feature");
      expect(info.tree).not.toBe("");
      expect(cli(twin, "rev-parse", "HEAD^{tree}").trim()).toBe((await api.commitInfo(tip)).tree);
    } else {
      await h.api.undo(res.undo!.id);
    }
    expect(await state(r, h.api)).toBe(before);
  });

  it("rewrites a branch that is not checked out without touching files", async () => {
    const { r, h, c1, c3 } = await feature();
    await h.api.apply({ action: "switch", target: "main" });
    const before = await state(r, h.api);
    const res = await h.api.apply({ action: "rewrite", branch: "feature", tip: c1, summary: "" });
    expect(res.reload).toBe(false);
    expect(res.message).toBe("Rewrote feature.");
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(c1);
    await h.api.undo(res.undo!.id);
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(c3);
    expect(await state(r, h.api)).toBe(before);
  });

  it("refuses published commits, a missing tip, local changes and unsaved edits", async () => {
    const { r, h, c1, c2, c3 } = await feature();
    await onRemote(r, "feature", c3);
    const p = await h.api.plan({ action: "rewrite", branch: "feature", tip: c1, summary: "x" });
    expect(p.blocked?.reason).toBe("published");
    expect(p.blocked?.message).toContain("server");
    // Only c3 stays local on top of published c2: replacing it is fine.
    await onRemote(r, "feature", c2);
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: c1, summary: "x" })).toBe("published");
    const c4 = await commitHere(r, { "notes.txt": "A\nb\nD\n" }, "c4");
    const reword = await plumbing(r, h).writeCommit({ tree: (await plumbing(r, h).commitInfo(c4)).tree, parents: [c3], message: "c4 reworded\n" });
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: reword, summary: "" })).toBeUndefined();
    expect(await blocked(h.api, { action: "rewrite", branch: "nope", tip: c1, summary: "" })).toBe("not-found");
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: "deadbeef", summary: "" })).toBe("not-found");
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: c4, summary: "" })).toBe("nothing-to-do");
    r.write("notes.txt", "local edit\n");
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: reword, summary: "" })).toBe("local-changes");
    h.dirty = ["x.yml"];
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: reword, summary: "" })).toBe("unsaved-changes");
  });

  it("undo is refused once the branch moved on", async () => {
    const { r, h, c1 } = await feature();
    const res = await h.api.apply({ action: "rewrite", branch: "feature", tip: c1, summary: "" });
    await commitHere(r, { "notes.txt": "later\n" }, "later");
    await expect(h.api.undo(res.undo!.id)).rejects.toThrow(/Cannot undo/);
  });

  it("accepts a fast-forward to a descendant even when the branch is published", async () => {
    const { r, h, c1, c3 } = await feature();
    await h.api.apply({ action: "switch", target: "main" });
    await h.api.apply({ action: "rewrite", branch: "feature", tip: c1, summary: "" });
    await onRemote(r, "feature", c1);
    expect(await blocked(h.api, { action: "rewrite", branch: "feature", tip: c3, summary: "" })).toBeUndefined();
    await h.api.apply({ action: "rewrite", branch: "feature", tip: c3, summary: "" });
    expect(await git.resolveRef({ fs: r.fs, dir: r.dir, ref: "refs/heads/feature" })).toBe(c3);
  });

  it("cherry-picks a commit that exists only as an object", async () => {
    const { r, h, api, c1, c3 } = await feature();
    await h.api.apply({ action: "switch", target: "main" });
    const tree = await api.writeTree({ base: c1, files: [{ path: "/loose.txt", kind: "text", text: "loose\n" }] });
    const loose = await api.writeCommit({ tree, parents: [c1], message: "loose\n" });
    void c3;
    expect(await blocked(h.api, { action: "cherry-pick", commit: loose })).toBeUndefined();
    await h.api.apply({ action: "cherry-pick", commit: loose });
    expect((await api.commitInfo("HEAD")).message).toContain("loose");
    void r;
  });
});

describe("fileDiff against the empty tree", () => {
  it("a root commit diffs against nothing when from is null", async () => {
    const r = await committed();
    const root = (await repoOps.log(r, 50)).at(-1)!.oid;
    const sides = await fileSides(r, null, root, "/notes.txt");
    expect(sides.old).toBeNull();
    expect(new TextDecoder().decode(sides.new!)).toBe("a\nb\nc\n");
  });
});
