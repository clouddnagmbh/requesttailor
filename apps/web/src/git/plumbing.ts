// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D: the commit building blocks of the web app (`GitApi.commitInfo`, `pick`,
// `writeTree`, `writeCommit`) and the result of the 3-way text conflict view (`resolveText`),
// on isomorphic-git, with the answers of `rt-git/src/plumbing.rs`.
//
// Nothing here moves a ref or touches the index; `pick` merges like cherry-pick and revert do
// (collection files on the model, every other file line by line) but writes no working file.
// Objects nothing points at are harmless. A history editor moves a branch only through the
// `rewrite` action (./actions.ts).

import {
  BackendError,
  type GitCommitDetails,
  type GitCommitWrite,
  type GitConflict,
  type GitIdentity,
  type GitPickRequest,
  type GitPickResult,
  type GitPlumbingMethods,
  type GitSignature,
  type GitTreeWrite,
} from "@requesttailor/ui";

import {
  b,
  blobText,
  computeMerge,
  type Entry,
  flatOfTree,
  type Flat,
  ready,
  readCommit,
  writeFlatTree,
} from "./actions";
import type { WebGitContext } from "./graph";
import type { Repo } from "./repo";
import * as repoOps from "./repo";
import { revParse } from "./revisions";
import { repoPath } from "./scope";

const gitErr = (message: string) => new BackendError("git", message);

type Iso = typeof import("isomorphic-git").default;
let isoLoading: Promise<Iso> | null = null;
function iso(): Promise<Iso> {
  isoLoading ??= (async () => {
    await ready();
    const m = (await import("isomorphic-git")) as unknown as { default?: Iso };
    return m.default ?? (m as unknown as Iso);
  })().catch((e: unknown) => {
    isoLoading = null;
    throw e;
  });
  return isoLoading;
}

/** What the building blocks need beyond the repository: the author identity for commits. */
export interface WebGitPlumbingContext extends WebGitContext {
  identity(): GitIdentity | null;
}

const toSignature = (s: { name: string; email: string; timestamp: number; timezoneOffset: number }): GitSignature => ({
  name: s.name,
  email: s.email,
  time: s.timestamp,
  // isomorphic-git counts minutes west of UTC (as `Date#getTimezoneOffset`), the API east.
  offset: 0 - s.timezoneOffset || 0,
});

const fromSignature = (s: GitSignature) => ({ name: s.name, email: s.email, timestamp: s.time, timezoneOffset: 0 - s.offset || 0 });

/** A tree id, or the tree of a commit (any rev). */
async function treeOf(r: Repo, git: Iso, rev: string): Promise<string> {
  const text = rev.trim();
  if (/^[0-9a-f]{40}$/.test(text)) {
    try {
      const { type } = await git.readObject({ ...b(r), oid: text, format: "parsed" });
      if (type === "tree") return text;
      if (type === "commit") return (await readCommit(r, text)).tree;
    } catch {
      // Not an object: maybe a branch of that name.
    }
  }
  return (await readCommit(r, await revParse(r, text))).tree;
}

/** The merge of `commit` onto `onto` (no files written). */
async function pick(r: Repo, git: Iso, request: GitPickRequest): Promise<GitPickResult> {
  const onto = await revParse(r, request.onto);
  const commit = await revParse(r, request.commit);
  const parents = (await readCommit(r, commit)).parent;
  let parent: string | null;
  if (parents.length <= 1) parent = parents[0] ?? null;
  else if (request.mainline === undefined) throw gitErr("This is a merge commit: say which parent counts as the mainline.");
  else if (request.mainline < 1 || request.mainline > parents.length) throw gitErr("The commit has no such parent.");
  else parent = parents[request.mainline - 1];
  if (request.revert && !parent) throw gitErr("The first commit cannot be reverted.");
  const merged = request.revert ? await computeMerge(r, commit, onto, parent!) : await computeMerge(r, parent, onto, commit);

  const entries: Flat = new Map(merged.ours);
  const conflicts: GitConflict[] = [];
  const other: string[] = [];
  for (const f of merged.files) {
    if (f.conflicts.length > 0) {
      // Our version stays in the tree; the three sides go to the view.
      const [be, oe, te] = f.stages ?? [];
      const sides = [be, oe, te];
      const isText = async (e: Entry | undefined) => e === undefined || (e.mode.startsWith("100") && (await blobText(r, e)) !== null);
      if (f.stages && (await Promise.all(sides.map(isText))).every(Boolean)) {
        conflicts.push({ path: `/${f.path}`, base: await blobText(r, be), ours: await blobText(r, oe), theirs: await blobText(r, te) });
      } else other.push(`/${f.path}`);
      continue;
    }
    if (f.content === null) entries.delete(f.path);
    else if ("entry" in f.content) entries.set(f.path, f.content.entry);
    else {
      const oid = await git.writeBlob({ fs: r.fs, dir: r.dir, blob: new TextEncoder().encode(f.content.text) });
      entries.set(f.path, { mode: merged.ours.get(f.path)?.mode ?? "100644", oid, type: "blob" });
    }
  }
  const tree = await writeFlatTree(r, entries);
  return {
    tree,
    clean: conflicts.length === 0 && other.length === 0,
    ...(conflicts.length ? { conflicts } : {}),
    ...(other.length ? { other_conflicts: other } : {}),
  };
}

async function writeTree(r: Repo, git: Iso, write: GitTreeWrite): Promise<string> {
  const flat = await flatOfTree(r, await treeOf(r, git, write.base));
  for (const file of write.files) {
    const rel = file.path.replace(/^\/+/, "");
    if (!rel || rel.split("/").some((part) => part === "" || part === "." || part === "..")) throw gitErr(`"${file.path}" is not a path in the repository.`);
    const existing = flat.get(rel);
    const mode = (from?: string) => (file.executable === true ? "100755" : file.executable === false ? "100644" : (from ?? "100644"));
    const put = async (blob: Uint8Array) => {
      flat.set(rel, { mode: mode(existing?.mode), oid: await git.writeBlob({ fs: r.fs, dir: r.dir, blob }), type: "blob" });
    };
    switch (file.kind) {
      case "text":
        await put(new TextEncoder().encode(file.text));
        break;
      case "base64": {
        let bytes: Uint8Array;
        try {
          bytes = Uint8Array.from(atob(file.data), (c) => c.charCodeAt(0));
        } catch {
          throw gitErr(`${file.path} holds invalid base64.`);
        }
        await put(bytes);
        break;
      }
      case "from": {
        const other = (await flatOfTree(r, await treeOf(r, git, file.rev))).get(rel);
        if (other) flat.set(rel, { ...other, mode: file.executable === undefined ? other.mode : mode(other.mode) });
        else flat.delete(rel);
        break;
      }
      case "delete":
        flat.delete(rel);
        break;
    }
  }
  return writeFlatTree(r, flat);
}

export function webGitPlumbing(ctx: WebGitPlumbingContext): GitPlumbingMethods {
  const open = async () => {
    const git = await iso();
    return { git, r: ctx.repo() };
  };
  return {
    commitInfo: (rev) =>
      ctx.run(async (): Promise<GitCommitDetails> => {
        const { git, r } = await open();
        const oid = await revParse(r, rev);
        const { commit } = await git.readCommit({ ...b(r), oid });
        return {
          oid,
          tree: commit.tree,
          parents: commit.parent,
          message: commit.message,
          author: toSignature(commit.author),
          committer: toSignature(commit.committer),
          signed: Boolean(commit.gpgsig),
        };
      }),

    pick: (request) =>
      ctx.run(async () => {
        const { git, r } = await open();
        return pick(r, git, request);
      }),

    writeTree: (write) =>
      ctx.run(async () => {
        const { git, r } = await open();
        return writeTree(r, git, write);
      }),

    writeCommit: (write: GitCommitWrite) =>
      ctx.run(async () => {
        const { git, r } = await open();
        const tree = write.tree.trim();
        try {
          await git.readTree({ ...b(r), oid: tree });
        } catch {
          throw gitErr(`The tree ${write.tree} does not exist.`);
        }
        const parent = await Promise.all(write.parents.map((p) => revParse(r, p)));
        const now = () => {
          const who = ctx.identity();
          if (!who) throw gitErr("Set your name and email for commits first.");
          const t = new Date();
          return { name: who.name, email: who.email, timestamp: Math.floor(t.getTime() / 1000), timezoneOffset: t.getTimezoneOffset() };
        };
        const committer = write.committer ? fromSignature(write.committer) : now();
        const author = write.author ? fromSignature(write.author) : committer;
        return git.writeCommit({ fs: r.fs, dir: r.dir, commit: { tree, parent, message: write.message, author, committer } });
      }),

    resolveText: (path, text) =>
      ctx.run(async () => {
        const { r } = await open();
        let dirty = 0;
        try {
          dirty = ctx.host.session().ws.dirty().length;
        } catch {
          dirty = 0; // No collection open: nothing unsaved.
        }
        if (dirty > 0) throw gitErr("Save or discard your changes before you resolve a conflict.");
        const index = await r.fs.promises.readFile(`${r.dir.replace(/\/+$/, "")}/.git/index`).catch(() => null);
        const unmerged = index ? repoOps.unmergedPaths(index as Uint8Array) : [];
        if (!unmerged.includes(repoPath(r, path))) throw gitErr(`${path} has no conflict.`);
        await repoOps.writeResolved(r, [[path, text]]);
        await ctx.host.openHandle(ctx.host.session().handle);
      }),
  };
}
