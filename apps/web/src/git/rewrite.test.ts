// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Secret rescue on isomorphic-git (wave 12, strand G, acceptance item 7), on Node's fs in a
// temporary folder, with the real core (rt-wasm from `node scripts/build-wasm.mjs`; without
// it the suite is skipped): the same repository as `crates/rt-git/tests/rewrite.rs` (a token
// in three commits on two branches), the same finding, and after the rewrite the same trees
// as the desktop's (`TREES`). The `git` command line, when installed, checks the objects.

import { execFileSync } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import git from "isomorphic-git";
import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { FsClient } from "./fs";
import * as repoOps from "./repo";
import type { Repo } from "./repo";
import { BACKUP_PREFIX, recommit, rewriteHistory, scanHistory, type RescueCore } from "./rewrite";

const WASM_JS = fileURLToPath(new URL("../../public/wasm/rt_wasm.js", import.meta.url));
const WASM_BG = fileURLToPath(new URL("../../public/wasm/rt_wasm_bg.wasm", import.meta.url));
const built = nodeFs.existsSync(WASM_JS) && nodeFs.existsSync(WASM_BG);

/** Root trees of `main` and `feature` after the rewrite: the desktop's
 *  (`crates/rt-git/tests/rewrite.rs`, `TREES`). */
const TREES = ["df5c807f6280c3bf3af57dece1f26fce124c4201", "102752dec589d21c9d0a916b79576b0924536fc0"];

/** Assembled at run time, so secret scanners of the repository stay quiet (as in the Rust test). */
const TOKEN = ["ghp_", "R3scueT0ken", "AbCdEfGhIjKlMnOpQrStUv", "0123"].join("");

const COLLECTION = "opencollection: 1.0.0\ninfo:\n  name: Rescue\n";
const request = (url: string, token: string) =>
  `info:\n  name: Get user\n  type: http\nhttp:\n  method: GET\n  url: ${url}\n  headers:\n    - name: Authorization\n      value: Bearer ${token}\n`;

const me = { name: "Ada", email: "ada@example.com" };
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

type TestRepo = Repo & { write(p: string, t: string): void; read(p: string): string };

function newRepo(): TestRepo {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-rescue-"));
  dirs.push(dir);
  return {
    fs: nodeFs as unknown as FsClient,
    dir,
    cache: {},
    write: (p, t) => {
      nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
      nodeFs.writeFileSync(join(dir, p), t);
    },
    read: (p) => nodeFs.readFileSync(join(dir, p), "utf8"),
  };
}

const ref = (r: Repo, name: string) => git.resolveRef({ fs: r.fs, dir: r.dir, ref: name });
const treeOf = async (r: Repo, oid: string) => (await git.readCommit({ fs: r.fs, dir: r.dir, oid })).commit.tree;

/** The repository of the Rust test: c1, c2 on `main`, c3 on `feature`; `main` has an
 *  upstream; tag `v1` on c1; `HEAD` is `main`. */
async function fixture() {
  const r = newRepo();
  await repoOps.init(r);
  r.write("opencollection.yml", COLLECTION);
  r.write("users/get-user.yml", request("https://api.example.com/users/1", TOKEN));
  r.write("docs/notes.yml", "info:\n  name: Notes\n");
  const c1 = await repoOps.commit(r, "Add the user request", ["opencollection.yml", "users/get-user.yml", "docs/notes.yml"], me);
  r.write("docs/notes.yml", "info:\n  name: Notes v2\n");
  const c2 = await repoOps.commit(r, "Notes", ["docs/notes.yml"], me);
  await repoOps.createBranch(r, "feature", true);
  r.write("users/get-user.yml", request("https://api.example.com/users/2", TOKEN));
  const c3 = await repoOps.commit(r, "Second user", ["users/get-user.yml"], me);
  await repoOps.checkout(r, "main");
  // As after a push: an upstream and its remote-tracking ref.
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "remote.origin.url", value: "https://git.example.com/acme/api.git" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.remote", value: "origin" });
  await git.setConfig({ fs: r.fs, dir: r.dir, path: "branch.main.merge", value: "refs/heads/main" });
  await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/remotes/origin/main", value: c2.oid });
  await git.writeRef({ fs: r.fs, dir: r.dir, ref: "refs/tags/v1", value: c1.oid });
  return { r, c1: c1.oid, c2: c2.oid, c3: c3.oid };
}

let core: RescueCore;

beforeAll(async () => {
  if (!built) return;
  const mod = (await import(/* @vite-ignore */ pathToFileURL(WASM_JS).href)) as RescueCore & {
    initSync(opts: { module: BufferSource }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(WASM_BG) });
  core = mod;
});

describe("recommit", () => {
  it("swaps tree and parents, keeps every other byte and drops signatures", () => {
    const enc = new TextEncoder();
    const raw = enc.encode(
      "tree aaaa\nparent bbbb\nauthor A <a@x> 1 +0200\ncommitter C <c@x> 2 -0100\ngpgsig -----BEGIN-----\n line\n -----END-----\nencoding ISO-8859-1\n\nSubject\n\nBody\n",
    );
    const out = new TextDecoder().decode(recommit(raw, "cccc", ["dddd", "eeee"]));
    expect(out).toBe(
      "tree cccc\nparent dddd\nparent eeee\nauthor A <a@x> 1 +0200\ncommitter C <c@x> 2 -0100\nencoding ISO-8859-1\n\nSubject\n\nBody\n",
    );
  });
});

describe.skipIf(!built)("secret rescue on isomorphic-git (real core)", () => {
  it("finds the token once, rewrites both branches like the desktop, and a new scan is clean", async () => {
    const { r, c1, c2, c3 } = await fixture();

    const progress: [number, number][] = [];
    const scan = await scanHistory(r, core, (p) => progress.push([p.scanned, p.total]));
    expect(scan.commits).toBe(3);
    expect(scan.findings).toHaveLength(1);
    const f = scan.findings[0];
    expect(f.id).toBe(`${c1}:users/get-user.yml:9`);
    expect(f.first.oid).toBe(c1);
    expect(f.commits).toEqual([c1, c2, c3]);
    expect(f.branches).toEqual(["feature", "main"]);
    expect(f.in_head).toBe(true);
    expect(JSON.stringify(scan)).not.toContain(TOKEN);
    expect(progress.at(-1)).toEqual([3, 3]);

    const plan = { findings: [f.id], variable: "githubToken", environment: "Local" };
    const stored: string[] = [];
    const store = async (v: string) => {
      stored.push(v);
    };

    // Uncommitted changes: refused, nothing stored, no backups.
    r.write("users/get-user.yml", "info:\n  name: edited\n");
    await expect(rewriteHistory(r, core, plan, 1_795_000_000, store)).rejects.toThrow(/Commit or discard/);
    expect(stored).toEqual([]);
    r.write("users/get-user.yml", request("https://api.example.com/users/1", TOKEN));

    const oldMain = await ref(r, "refs/heads/main");
    const oldFeature = await ref(r, "refs/heads/feature");
    const result = await rewriteHistory(r, core, plan, 1_795_000_000, store);
    expect(stored).toEqual([TOKEN]);
    expect(JSON.stringify(result)).not.toContain(TOKEN);
    expect(result.rewritten).toBe(3);
    expect(result.backup).toBe(`${BACKUP_PREFIX}1795000000/`);
    expect(result.tags).toEqual(["v1"]);
    expect(result.branches.map((b) => [b.name, b.pushed, b.remote])).toEqual([
      ["feature", false, null],
      ["main", true, "origin"],
    ]);
    expect(await ref(r, `${result.backup}main`)).toBe(oldMain);
    expect(await ref(r, `${result.backup}feature`)).toBe(oldFeature);
    expect(await ref(r, "refs/remotes/origin/main")).toBe(oldMain);
    expect(await ref(r, "refs/tags/v1")).toBe(c1);

    // The same trees as the desktop's rewrite.
    const [main, feature] = result.branches[1].name === "main" ? [result.branches[1], result.branches[0]] : [result.branches[0], result.branches[1]];
    expect([await treeOf(r, main.new), await treeOf(r, feature.new)]).toEqual(TREES);

    // Clean afterwards; the work tree follows HEAD.
    expect((await scanHistory(r, core)).findings).toEqual([]);
    expect(r.read("users/get-user.yml")).toContain("value: Bearer {{githubToken}}");
    expect((await repoOps.status(r)).files).toEqual([]);
    const log = await git.log({ fs: r.fs, dir: r.dir, ref: "feature" });
    expect(log.map((c) => c.commit.message.trim())).toEqual(["Second user", "Notes", "Add the user request"]);
    if (hasGit) {
      execFileSync("git", ["fsck", "--strict", "--no-dangling"], {
        cwd: r.dir,
        env: { ...process.env, GIT_CONFIG_GLOBAL: "/dev/null" },
      });
    }

    // Nothing left to rescue.
    await expect(rewriteHistory(r, core, plan, 1_795_000_000, store)).rejects.toThrow(/scan the history again/);
  });

  it("refuses invalid plans and a detached HEAD before anything happens", async () => {
    const { r, c1 } = await fixture();
    const store = async () => {
      throw new Error("must not store");
    };
    const id = `${c1}:users/get-user.yml:9`;
    await expect(rewriteHistory(r, core, { findings: [id], variable: "not a name", environment: "Local" }, 1, store)).rejects.toThrow();
    await expect(rewriteHistory(r, core, { findings: [], variable: "t", environment: "Local" }, 1, store)).rejects.toThrow();
    await expect(
      rewriteHistory(r, core, { findings: [`${c1}:users/get-user.yml:2`], variable: "t", environment: "Local" }, 1, store),
    ).rejects.toThrow(/scan the history again/);
    await git.checkout({ fs: r.fs, dir: r.dir, ref: c1 });
    await expect(rewriteHistory(r, core, { findings: [id], variable: "t", environment: "Local" }, 1, store)).rejects.toThrow(
      /Switch to a branch/,
    );
  });
});
