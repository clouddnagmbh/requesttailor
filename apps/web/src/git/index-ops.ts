// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand F: the index as building blocks of the web app (`GitApi.indexDiff`, `stage`,
// `unstage`, `stagePatch`, `unstagePatch`, `discardPatch`, `commitStaged`), signature checks
// (`verify`) and `config`, on isomorphic-git's index, with the answers of
// `rt-git/src/{index_ops,patch}.rs`.
//
// - Diffs come from the core (`textDiff`), lines are chosen by `applyLineSelection` (the same
//   rules as `patch.rs`); a selection carries the blob ids of both sides and is refused when
//   either changed since. Lines are chosen on the raw file bytes.
// - Index entries written for a chosen part carry zero stats (`updateIndex` with an `oid`), so
//   `statusMatrix` looks at the file again instead of believing it is unchanged.
// - Signing is native only: the web app never signs, and `verify` answers `unverified` for a
//   commit with a `gpgsig` header and `unsigned` for the rest (no verifier in the browser).
// - `config` reads the repository's own configuration (the browser has no global one).
// - `commitStaged` runs the secret scanner over the staged files first (`secrets-found`).

import {
  applyLineSelection,
  BackendError,
  chosenLines,
  type GitCommit,
  type GitIdentity,
  type GitIndexDiff,
  type GitIndexMethods,
  type GitPatchRequest,
  type GitPatchSide,
  type GitSignatureCheck,
  type GitStagedCommit,
} from "@requesttailor/ui";

import { b, ready, writeFlatTree, type Entry, type Flat } from "./actions";
import type { WebGitContext } from "./graph";
import { unmergedPaths, type Repo } from "./repo";
import { revParse } from "./revisions";
import { repoPath, collectionPath } from "./scope";

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

/** What the index and the identity of commits need beyond the repository. */
export interface WebGitIndexContext extends WebGitContext {
  identity(): GitIdentity | null;
}

/** One side of a file. */
interface Side {
  oid: string | null;
  /** Octal mode string as in a tree (`100644`). */
  mode: string | null;
  bytes: Uint8Array | null;
}

const NONE: Side = { oid: null, mode: null, bytes: null };

const utf8 = (bytes: Uint8Array | null): string => (bytes ? new TextDecoder("utf-8", { fatal: true }).decode(bytes) : "");

const modeNumber = (mode: string | null): number => (mode ? Number.parseInt(mode, 8) : 0o100644);

function isMissing(e: unknown): boolean {
  const code = (e as { code?: string }).code;
  return code === "ENOENT" || code === "ENOTDIR" || code === "NotFoundError";
}

/** The blob at `filepath` in a walked tree (`git.STAGE()` or `git.TREE({ ref })`), or `null`. */
async function entryIn(git: Iso, r: Repo, tree: ReturnType<Iso["STAGE"]>, filepath: string): Promise<{ oid: string; mode: string } | null> {
  type Hit = { oid: string; mode: string };
  const found = await git.walk({
    ...b(r),
    trees: [tree],
    map: async (path, [entry]) => {
      if (path === ".") return undefined;
      if (path === filepath) {
        if (!entry) return null;
        const type = await entry.type();
        if (type !== "blob" && type !== "commit") return null;
        return { oid: await entry.oid(), mode: (await entry.mode()).toString(8) } as Hit;
      }
      // Only the folders on the way to the file.
      return filepath.startsWith(`${path}/`) ? undefined : null;
    },
    reduce: async (parent: Hit | undefined, children: unknown[]) => {
      const nested = children.flat(Infinity) as (Hit | undefined)[];
      return parent ?? nested.find((c) => c !== undefined && c !== null);
    },
  });
  const hit = (Array.isArray(found) ? found.flat(Infinity) : [found]).find((x): x is Hit => Boolean(x));
  return hit ?? null;
}

async function blobSide(git: Iso, r: Repo, hit: { oid: string; mode: string } | null): Promise<Side> {
  if (!hit) return NONE;
  if (hit.mode === "160000") return { oid: hit.oid, mode: hit.mode, bytes: null };
  const { blob } = await git.readBlob({ ...b(r), oid: hit.oid });
  return { oid: hit.oid, mode: hit.mode, bytes: blob };
}

async function indexSide(git: Iso, r: Repo, filepath: string): Promise<Side> {
  return blobSide(git, r, await entryIn(git, r, git.STAGE(), filepath));
}

async function headSide(git: Iso, r: Repo, filepath: string): Promise<Side> {
  try {
    await git.resolveRef({ ...b(r), ref: "HEAD" });
  } catch {
    return NONE;
  }
  return blobSide(git, r, await entryIn(git, r, git.TREE({ ref: "HEAD" }), filepath));
}

async function workSide(git: Iso, r: Repo, filepath: string): Promise<Side> {
  let bytes: Uint8Array;
  try {
    bytes = (await r.fs.promises.readFile(`${r.dir.replace(/\/+$/, "")}/${filepath}`)) as Uint8Array;
  } catch (e) {
    if (isMissing(e)) return NONE;
    if ((e as { code?: string }).code === "EISDIR") throw gitErr(`${filepath} is a folder, not a file.`);
    throw e;
  }
  const { oid } = await git.hashBlob({ object: bytes });
  return { oid, mode: "100644", bytes };
}

async function refuseConflict(r: Repo, filepath: string, shown: string): Promise<void> {
  try {
    const index = (await r.fs.promises.readFile(`${r.dir.replace(/\/+$/, "")}/.git/index`)) as Uint8Array;
    if (unmergedPaths(index).includes(filepath)) throw gitErr(`${shown} has a conflict: resolve it first.`);
  } catch (e) {
    if (!isMissing(e)) throw e;
  }
}

async function sides(git: Iso, r: Repo, filepath: string, shown: string, side: GitPatchSide): Promise<{ old: Side; new: Side }> {
  await refuseConflict(r, filepath, shown);
  if (side === "unstaged") return { old: await indexSide(git, r, filepath), new: await workSide(git, r, filepath) };
  return { old: await headSide(git, r, filepath), new: await indexSide(git, r, filepath) };
}

/** Everything the index holds (conflict-free) as a flat tree. */
async function indexFlat(git: Iso, r: Repo): Promise<Flat> {
  const out: Flat = new Map();
  await git.walk({
    ...b(r),
    trees: [git.STAGE()],
    map: async (path, [entry]) => {
      if (path === "." || !entry) return undefined;
      const type = await entry.type();
      if (type === "tree") return undefined;
      out.set(path, { mode: (await entry.mode()).toString(8), oid: await entry.oid(), type: type as Entry["type"] });
      return undefined;
    },
  });
  return out;
}

async function headFlat(git: Iso, r: Repo): Promise<Flat> {
  const out: Flat = new Map();
  try {
    await git.resolveRef({ ...b(r), ref: "HEAD" });
  } catch {
    return out;
  }
  await git.walk({
    ...b(r),
    trees: [git.TREE({ ref: "HEAD" })],
    map: async (path, [entry]) => {
      if (path === "." || !entry) return undefined;
      const type = await entry.type();
      if (type === "tree") return undefined;
      out.set(path, { mode: (await entry.mode()).toString(8), oid: await entry.oid(), type: type as Entry["type"] });
      return undefined;
    },
  });
  return out;
}

async function setEntry(git: Iso, r: Repo, filepath: string, entry: { oid: string; mode: string } | null): Promise<void> {
  if (!entry) {
    await git.updateIndex({ ...b(r), filepath, remove: true, force: true });
    return;
  }
  await git.updateIndex({ ...b(r), filepath, oid: entry.oid, mode: modeNumber(entry.mode), add: true });
}

async function writeBlob(git: Iso, r: Repo, text: string): Promise<string> {
  return git.writeBlob({ fs: r.fs, dir: r.dir, blob: new TextEncoder().encode(text) });
}

export function webGitIndex(ctx: WebGitIndexContext): GitIndexMethods {
  const open = async () => ({ git: await iso(), r: ctx.repo() });

  /** Both sides of `patch`, refused when stale, and the chosen lines mapped onto them. */
  async function prepare(patch: GitPatchRequest, side: GitPatchSide) {
    const { git, r } = await open();
    if (patch.side !== side) throw gitErr("This change works on the other side of the file.");
    const filepath = repoPath(r, patch.path);
    const shown = collectionPath(r, filepath);
    const s = await sides(git, r, filepath, shown, side);
    if ((s.old.oid ?? "") !== patch.old || (s.new.oid ?? "") !== patch.new) {
      throw gitErr(`${shown} changed since you chose these lines: reload it and choose again.`);
    }
    const core = await ctx.host.deps.loadCore();
    const diff = core.textDiff(shown, s.old.bytes, s.new.bytes);
    if (diff.binary) throw gitErr(`${shown} is not a text file: choose it as a whole.`);
    const chosen = chosenLines(diff, patch.hunks);
    const oldText = utf8(s.old.bytes);
    const newText = utf8(s.new.bytes);
    return { git, r, filepath, s, diff, chosen, oldText, newText };
  }

  return {
    indexDiff: (path, side): Promise<GitIndexDiff> =>
      ctx.run(async () => {
        const { git, r } = await open();
        const filepath = repoPath(r, path);
        const shown = collectionPath(r, filepath);
        const s = await sides(git, r, filepath, shown, side);
        const core = await ctx.host.deps.loadCore();
        return { side, diff: core.textDiff(shown, s.old.bytes, s.new.bytes), old: s.old.oid ?? "", new: s.new.oid ?? "" };
      }),

    stage: (paths) =>
      ctx.run(async () => {
        const { git, r } = await open();
        for (const path of paths) {
          const filepath = repoPath(r, path);
          const shown = collectionPath(r, filepath);
          const work = await workSide(git, r, filepath);
          if (work.bytes) {
            await git.add({ ...b(r), filepath });
          } else if (await indexSide(git, r, filepath).then((s) => s.oid !== null)) {
            await git.remove({ fs: r.fs, dir: r.dir, filepath });
          } else {
            throw gitErr(`${shown} is neither in the working tree nor staged.`);
          }
        }
      }),

    unstage: (paths) =>
      ctx.run(async () => {
        const { git, r } = await open();
        for (const path of paths) await git.resetIndex({ ...b(r), filepath: repoPath(r, path) });
      }),

    stagePatch: (patch) =>
      ctx.run(async () => {
        const { git, r, filepath, s, chosen, oldText, newText, diff } = await prepare(patch, "unstaged");
        if (chosen.size === 0) return;
        const result = applyLineSelection(oldText, newText, diff, chosen);
        // Every line of a deleted file: the deletion.
        if (s.new.bytes === null && result === "") return setEntry(git, r, filepath, null);
        const oid = await writeBlob(git, r, result);
        await setEntry(git, r, filepath, { oid, mode: s.old.mode ?? s.new.mode ?? "100644" });
      }),

    unstagePatch: (patch) =>
      ctx.run(async () => {
        const { git, r, filepath, s, chosen, oldText, newText, diff } = await prepare(patch, "staged");
        if (chosen.size === 0) return;
        const result = applyLineSelection(oldText, newText, diff, chosen, true);
        if (s.old.bytes !== null && result === oldText) {
          // Everything unstaged: exactly what HEAD has.
          return setEntry(git, r, filepath, { oid: s.old.oid!, mode: s.old.mode! });
        }
        if (s.old.bytes === null && result === "") return setEntry(git, r, filepath, null);
        const oid = await writeBlob(git, r, result);
        await setEntry(git, r, filepath, { oid, mode: s.new.mode ?? s.old.mode ?? "100644" });
      }),

    discardPatch: (patch) =>
      ctx.run(async () => {
        const { git, r, filepath, s, chosen, oldText, newText, diff } = await prepare(patch, "unstaged");
        if (chosen.size === 0) return;
        if (s.old.bytes === null) throw gitErr(`${collectionPath(r, filepath)} is not tracked yet: there is nothing to go back to.`);
        const result = applyLineSelection(oldText, newText, diff, chosen, true);
        // The file as it was is kept under a backup ref (a commit of HEAD with this one file),
        // as natively.
        await backUpFile(git, r, filepath, s.new);
        await writeWork(r, filepath, new TextEncoder().encode(result));
      }),

    commitStaged: (commit: GitStagedCommit): Promise<GitCommit> =>
      ctx.run(async () => {
        const { git, r } = await open();
        const message = commit.message.trim();
        if (!message) throw gitErr("The commit message is empty.");
        const who = ctx.identity();
        if (!who) throw gitErr("Set your name and email for commits first.");
        const mergeHead = await readText(r, ".git/MERGE_HEAD");
        const index = await readBytes(r, ".git/index");
        if (index && unmergedPaths(index).length > 0) throw gitErr("Resolve all conflicts before committing.");
        const staged = await indexFlat(git, r);
        const head = await headFlat(git, r);
        const tree = await writeFlatTree(r, staged);
        const headOid = await git.resolveRef({ ...b(r), ref: "HEAD" }).catch(() => null);
        const headTree = headOid ? (await git.readCommit({ ...b(r), oid: headOid })).commit.tree : null;
        const merging = mergeHead ? mergeHead.trim().split(/\s+/)[0] : null;
        if (tree === headTree && !merging) throw gitErr("Nothing is staged.");
        // The secret scanner first (as `commit`): the staged files that differ from HEAD.
        const core = await ctx.host.deps.loadCore();
        const texts: [string, string][] = [];
        for (const [path, e] of staged) {
          if (head.get(path)?.oid === e.oid || e.type === "commit") continue;
          const { blob } = await git.readBlob({ ...b(r), oid: e.oid });
          const text = scanText(blob);
          if (text !== null) texts.push([collectionPath(r, path), text]);
        }
        const findings = core.scanSecrets(texts);
        if (findings.length > 0) throw new BackendError("secrets-found", core.secretRefusal(findings));
        const person = { name: who.name, email: who.email };
        const oid = await git.commit({
          ...b(r),
          message,
          author: person,
          committer: person,
          tree,
          parent: [...(headOid ? [headOid] : []), ...(merging ? [merging] : [])],
        });
        if (merging) {
          for (const name of ["MERGE_HEAD", "MERGE_MSG", "MERGE_MODE"]) await removeFile(r, `.git/${name}`);
        }
        const { commit: c } = await git.readCommit({ ...b(r), oid });
        return { oid, summary: c.message.split("\n")[0], author: c.author.name, time: c.author.timestamp };
      }),

    verify: (oids): Promise<GitSignatureCheck[]> =>
      ctx.run(async () => {
        const { git, r } = await open();
        const out: GitSignatureCheck[] = [];
        for (const rev of oids) {
          const oid = await revParse(r, rev);
          const { commit } = await git.readCommit({ ...b(r), oid });
          out.push(unverified(oid, commit.gpgsig ?? null));
        }
        return out;
      }),

    config: (key) =>
      ctx.run(async () => {
        const { git, r } = await open();
        const value: unknown = await git.getConfig({ ...b(r), path: key }).catch(() => undefined);
        return typeof value === "string" ? value : null;
      }),
  };
}

/** `rt_app::git::signing::unverified`: the answer without a verifier. */
export function unverified(oid: string, signature: string | null): GitSignatureCheck {
  if (signature === null) return { oid, state: "unsigned" };
  const first = signature.trimStart().split("\n")[0].trim();
  const kind =
    first === "-----BEGIN PGP SIGNATURE-----"
      ? "gpg"
      : first === "-----BEGIN SSH SIGNATURE-----"
        ? "ssh"
        : first === "-----BEGIN SIGNED MESSAGE-----" || first === "-----BEGIN PKCS7-----"
          ? "x509"
          : undefined;
  return { oid, state: "unverified", ...(kind ? { kind } : {}) };
}

/** Text the secret scanner reads (`rt_app::secret_scan::text_of`). */
function scanText(bytes: Uint8Array): string | null {
  if (bytes.length > 4 * 1024 * 1024 || bytes.includes(0)) return null;
  try {
    return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(bytes);
  } catch {
    return null;
  }
}

const full = (r: Repo, rel: string) => `${r.dir.replace(/\/+$/, "")}/${rel}`;

async function readBytes(r: Repo, rel: string): Promise<Uint8Array | null> {
  try {
    return (await r.fs.promises.readFile(full(r, rel))) as Uint8Array;
  } catch (e) {
    if (isMissing(e)) return null;
    throw e;
  }
}

async function readText(r: Repo, rel: string): Promise<string | null> {
  const bytes = await readBytes(r, rel);
  return bytes ? new TextDecoder().decode(bytes) : null;
}

async function removeFile(r: Repo, rel: string): Promise<void> {
  try {
    await r.fs.promises.unlink(full(r, rel));
  } catch (e) {
    if (!isMissing(e)) throw e;
  }
}

async function writeWork(r: Repo, rel: string, data: Uint8Array): Promise<void> {
  const parts = rel.split("/");
  let dir = r.dir.replace(/\/+$/, "");
  for (const part of parts.slice(0, -1)) {
    dir = `${dir}/${part}`;
    try {
      await r.fs.promises.mkdir(dir);
    } catch (e) {
      if ((e as { code?: string }).code !== "EEXIST") throw e;
    }
  }
  await r.fs.promises.writeFile(full(r, rel), data);
}

/** A commit of HEAD with `filepath` as `side` held it, under `refs/requesttailor/backup/<unix>/discard`. */
async function backUpFile(git: Iso, r: Repo, filepath: string, side: Side): Promise<void> {
  const head = await git.resolveRef({ ...b(r), ref: "HEAD" }).catch(() => null);
  const flat = await headFlat(git, r);
  if (side.oid) flat.set(filepath, { mode: side.mode ?? "100644", oid: await git.writeBlob({ fs: r.fs, dir: r.dir, blob: side.bytes! }), type: "blob" });
  else flat.delete(filepath);
  const tree = await writeFlatTree(r, flat);
  const person = { name: "RequestTailor", email: "requesttailor@localhost" };
  const now = Math.floor(Date.now() / 1000);
  const stamp = { ...person, timestamp: now, timezoneOffset: 0 };
  const oid = await git.writeCommit({
    fs: r.fs,
    dir: r.dir,
    commit: { message: `Backup before discarding lines of ${filepath}\n`, tree, parent: head ? [head] : [], author: stamp, committer: stamp },
  });
  for (let n = 0; ; n++) {
    const ref = `refs/requesttailor/backup/${now}/${n === 0 ? "discard" : `discard-${n}`}`;
    const taken = await git.resolveRef({ ...b(r), ref }).then(
      () => true,
      () => false,
    );
    if (!taken) {
      await git.writeRef({ ...b(r), ref, value: oid });
      return;
    }
  }
}
