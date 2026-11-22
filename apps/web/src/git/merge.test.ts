// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web pull's semantic merge (wave 12, strand B) on Node's fs in a temporary folder. The
// merge itself is the core's (`rt_format::merge`, tested in Rust); here a stand-in merges
// `key: value` lines per key, so the tests check what the shell does around it: that
// isomorphic-git's merge driver answers with the core's result for the right file, that a
// rename the core resolves no longer stops the pull, that field conflicts stay conflicted in
// the index, and that `mergeSides`/`writeResolved` serve `MergeApi`.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import type { FieldConflict } from "@requesttailor/ui";
import git from "isomorphic-git";
import { afterEach, describe, expect, it } from "vitest";

import type { CollectionMerge, CollectionMerger, TextFiles } from "../merges";
import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";

const me = { name: "Test User", email: "test@example.com" };

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

type TestRepo = Repo & { write(p: string, t: string): void; read(p: string): string | null; rm(p: string): void };

function newRepo(files: Record<string, string>): TestRepo {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-merge-"));
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

async function committed(): Promise<TestRepo> {
  const r = newRepo({ "opencollection.yml": COLLECTION, "users/list.yml": REQUEST, "notes.txt": "a\nb\nc\n" });
  await repoOps.init(r);
  await repoOps.commit(r, "first", ["opencollection.yml", "users/list.yml", "notes.txt"], me);
  return r;
}

async function setUpstream(r: Repo, oid: string) {
  await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: oid, force: true });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "remote.origin.url", value: "https://git.example/r.git" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
}

/** A commit on a scratch branch (HEAD stays): `null` deletes a file. */
async function commitElsewhere(r: TestRepo, files: Record<string, string | null>) {
  await repoOps.createBranch(r, "scratch", true);
  for (const [p, t] of Object.entries(files)) if (t === null) r.rm(p); else r.write(p, t);
  const c = await repoOps.commit(r, "theirs", Object.keys(files), me);
  await repoOps.checkout(r, "main");
  await git.deleteBranch({ fs: r.fs, dir: r.dir, ref: "scratch" });
  return c.oid;
}

async function commitHere(r: TestRepo, files: Record<string, string | null>) {
  for (const [p, t] of Object.entries(files)) if (t === null) r.rm(p); else r.write(p, t);
  await repoOps.commit(r, "ours", Object.keys(files), me);
}

// The stand-in core: a file is `key: value` lines; keys merge one by one; a request that
// moved on one side (same text at a new path) takes the other side's changes.
const parse = (t: string) => new Map(t.split("\n").filter(Boolean).map((l) => [l.slice(0, l.indexOf(":")), l] as const));

function mergeOne(path: string, b: string | null, o: string, t: string): { text: string; conflicts: FieldConflict[]; marked: string } {
  const [mb, mo, mt] = [parse(b ?? ""), parse(o), parse(t)];
  const keys = [...new Set([...mo.keys(), ...mt.keys()])];
  const out: string[] = [];
  const marked: string[] = [];
  const conflicts: FieldConflict[] = [];
  for (const k of keys) {
    const [vb, vo, vt] = [mb.get(k), mo.get(k), mt.get(k)];
    const pick = vo === vt || vt === vb ? vo : vo === vb ? vt : undefined;
    if (pick === undefined) {
      conflicts.push({ target: { kind: "request", path }, file: path, field: [k], base: vb === undefined ? null : { kind: "text", text: vb }, ours: { kind: "text", text: vo ?? "" }, theirs: { kind: "text", text: vt ?? "" } });
      out.push(vo!);
      marked.push(`# conflict in ${k}`, vo!);
    } else if (pick !== undefined) {
      out.push(pick);
      marked.push(pick);
    }
  }
  return { text: out.map((l) => `${l}\n`).join(""), conflicts, marked: marked.map((l) => `${l}\n`).join("") };
}

const fake = {
  calls: 0,
  async merge(base: TextFiles, ours: TextFiles, theirs: TextFiles): Promise<CollectionMerge> {
    this.calls++;
    const [b, o, t] = [new Map(base), new Map(ours), new Map(theirs)];
    const files = new Map<string, string>();
    const marked: TextFiles = [];
    const conflicts: FieldConflict[] = [];
    const moves = new Map<string, string>();
    for (const [p, text] of b) {
      if (!o.has(p) && t.has(p)) {
        const to = [...o].find(([q, x]) => !b.has(q) && x === text)?.[0];
        if (to) moves.set(p, to);
      }
    }
    for (const p of new Set([...b.keys(), ...o.keys(), ...t.keys()])) {
      if (moves.has(p)) continue;
      const from = [...moves].find(([, to]) => to === p)?.[0];
      const [vb, vo, vt] = from ? [b.get(from)!, o.get(p)!, t.get(from)!] : [b.get(p), o.get(p), t.get(p)];
      if (vo === undefined || vt === undefined) {
        const keep = vo === vb ? vt : vo;
        if (keep !== undefined) files.set(p, keep);
        continue;
      }
      const m = mergeOne(p, vb ?? null, vo, vt);
      files.set(p, m.text);
      if (m.conflicts.length > 0) {
        conflicts.push(...m.conflicts);
        marked.push([p, m.marked]);
      }
    }
    return { files: [...files], conflicts, text_conflicts: [], marked };
  },
  async text(base: string | null, ours: string, theirs: string) {
    const [b, o, t] = [(base ?? "").split("\n"), ours.split("\n"), theirs.split("\n")];
    const lines = o.map((l, i) => (l === b[i] ? t[i] : l));
    return { text: lines.join("\n"), clean: true };
  },
} satisfies CollectionMerger & { calls: number };

const states = async (r: Repo) => (await repoOps.status(r)).files.map((f) => `${f.state} ${f.path}`);

describe("the semantic pull", () => {
  it("merges different fields on neighbouring lines and other files line by line", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, {
      "users/list.yml": REQUEST.replace("GET", "POST"),
      "notes.txt": "a\nb\nC\n",
    });
    await commitHere(r, { "users/list.yml": REQUEST.replace("/users", "/v2/users"), "notes.txt": "A\nb\nc\n" });
    await setUpstream(r, theirs);

    const result = await repoOps.mergeUpstream(r, me, fake);
    expect(result.outcome).toBe("merged");
    expect(r.read("users/list.yml")).toBe("method: POST\nurl: https://api.test/v2/users\ndocs: Lists the users.\n");
    expect(r.read("notes.txt")).toBe("A\nb\nC\n");
    expect(await states(r)).toEqual([]);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: (result as { oid: string }).oid });
    expect(commit.parent).toHaveLength(2);
  });

  it("takes theirs' change into a request ours renamed, and commits the merge", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "users/list.yml": REQUEST.replace("GET", "POST"), "extra.yml": "x: 1\n" });
    await commitHere(r, { "users/list.yml": null, "users/all.yml": REQUEST });
    await setUpstream(r, theirs);

    const result = await repoOps.mergeUpstream(r, me, fake);
    expect(result.outcome).toBe("merged");
    expect(r.read("users/list.yml")).toBeNull();
    expect(r.read("users/all.yml")).toBe(REQUEST.replace("GET", "POST"));
    expect(r.read("extra.yml")).toBe("x: 1\n");
    expect(await states(r)).toEqual([]);
    expect((await repoOps.status(r)).merging).toBe(false);
  });

  it("stops on the same field; MergeApi's sides and resolution finish the merge", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, {
      "users/list.yml": REQUEST.replace("GET", "POST").replace("api.test", "theirs.test"),
    });
    await commitHere(r, { "users/list.yml": REQUEST.replace("api.test", "ours.test") });
    await setUpstream(r, theirs);

    expect(await repoOps.mergeUpstream(r, me, fake)).toEqual({ outcome: "conflicts", paths: ["users/list.yml"] });
    // Ours in the conflicted field, theirs' method merged in, a comment at the field.
    expect(r.read("users/list.yml")).toBe(
      "method: POST\n# conflict in url\nurl: https://ours.test/users\ndocs: Lists the users.\n",
    );
    expect(await states(r)).toEqual(["conflicted users/list.yml"]);

    const sides = await repoOps.mergeSides(r);
    expect(sides.merging).toBe(true);
    expect(sides.context.map(([p]) => p)).toEqual(["opencollection.yml", "users/list.yml"]);
    expect(sides.stages).toEqual([
      {
        path: "users/list.yml",
        base: REQUEST,
        ours: REQUEST.replace("api.test", "ours.test"),
        theirs: REQUEST.replace("GET", "POST").replace("api.test", "theirs.test"),
      },
    ]);

    const resolved = REQUEST.replace("GET", "POST").replace("api.test", "theirs.test");
    await repoOps.writeResolved(r, [["users/list.yml", resolved]]);
    expect(r.read("users/list.yml")).toBe(resolved);
    expect(await states(r)).not.toContain("conflicted users/list.yml");
    expect((await repoOps.mergeSides(r)).stages).toEqual([]);
    const merge = await repoOps.commit(r, "Merge origin/main into main", ["users/list.yml"], me);
    const { commit } = await git.readCommit({ fs: r.fs, dir: r.dir, oid: merge.oid });
    expect(commit.parent).toHaveLength(2);
    expect(await repoOps.status(r)).toMatchObject({ merging: false, files: [] });
  });

  it("without a merger, isomorphic-git merges as before", async () => {
    const r = await committed();
    const theirs = await commitElsewhere(r, { "users/list.yml": REQUEST.replace("GET", "POST") });
    await commitHere(r, { "users/list.yml": REQUEST.replace("/users", "/v2/users") });
    await setUpstream(r, theirs);
    const result = await repoOps.mergeUpstream(r, me, null);
    // Neighbouring lines: a line conflict for Git's text merge.
    expect(result).toEqual({ outcome: "conflicts", paths: ["users/list.yml"] });
  });
});
