// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand F: the index on isomorphic-git (Node's fs, a temporary folder), the same cases
// and the same results as crates/rt-git/tests/{index_ops,patch}.rs: staging, unstaging and
// discarding chosen lines give the index and files `git apply` gives (compared with the `git`
// command line when installed), a stale selection is refused, `commitStaged` commits the index
// and scans it for credentials, `verify` and `config`. Needs the real core's line diff (rt-wasm
// from `node scripts/build-wasm.mjs`); without it these tests are skipped.

import { execFileSync } from "node:child_process";
import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { afterEach, beforeAll, describe, expect, it } from "vitest";

import type { GitIndexMethods, GitPatchRequest, GitPatchSide } from "@requesttailor/ui";

import type { WebHost } from "../web-backend";
import type { FsClient } from "./fs";
import type { WebGitContext } from "./graph";
import { unverified, webGitIndex } from "./index-ops";
import type { Repo } from "./repo";

const WASM_JS = fileURLToPath(new URL("../../public/wasm/rt_wasm.js", import.meta.url));
const WASM_BG = fileURLToPath(new URL("../../public/wasm/rt_wasm_bg.wasm", import.meta.url));
const built = nodeFs.existsSync(WASM_JS) && nodeFs.existsSync(WASM_BG);

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

const ENV = { ...process.env, GIT_CONFIG_NOSYSTEM: "1", GIT_CONFIG_GLOBAL: "/dev/null" };
const cli = (dir: string, ...args: string[]) => execFileSync("git", ["-C", dir, ...args], { encoding: "utf8", env: ENV });

type Core = {
  textDiff(path: string, old: Uint8Array | null, current: Uint8Array | null): ReturnType<typeof Object>;
  scanSecrets(files: [string, string][]): unknown[];
  secretRefusal(findings: unknown[]): string;
};
let core: Core | null = null;
beforeAll(async () => {
  if (!built) return;
  const mod = (await import(/* @vite-ignore */ pathToFileURL(WASM_JS).href)) as { default(input: unknown): Promise<unknown> };
  await mod.default({ module_or_path: nodeFs.readFileSync(WASM_BG) });
  core = mod as unknown as Core;
});

const long = () => Array.from({ length: 30 }, (_, i) => `l${String(i + 1).padStart(2, "0")}\n`).join("");
/** Line 1 replaced, a line added after line 3, line 30 replaced: two hunks. */
const edited = () => long().replace("l01\n", "L01\n").replace("l03\n", "l03\nl03b\n").replace("l30\n", "L30\n");

interface Fixture {
  dir: string;
  repo: Repo;
  api: GitIndexMethods;
  write(rel: string, text: string): void;
  read(rel: string): string;
}

function fixture(identity: { name: string; email: string } | null = { name: "Ada", email: "ada@example.com" }): Fixture {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-index-"));
  dirs.push(dir);
  cli(dir, "init", "-q", "-b", "main");
  cli(dir, "config", "user.name", "Ada");
  cli(dir, "config", "user.email", "ada@example.com");
  nodeFs.writeFileSync(join(dir, "long.txt"), long());
  nodeFs.writeFileSync(join(dir, "notes.txt"), "one\ntwo\n");
  cli(dir, "add", ".");
  cli(dir, "commit", "-q", "-m", "init");
  const repo: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  const host = { deps: { loadCore: async () => core } } as unknown as WebHost;
  const ctx = { host, repo: () => repo, run: <T>(op: () => Promise<T>) => op(), identity: () => identity } as WebGitContext & { identity(): typeof identity };
  return {
    dir,
    repo,
    api: webGitIndex(ctx as Parameters<typeof webGitIndex>[0]),
    write: (rel, text) => nodeFs.writeFileSync(join(dir, rel), text),
    read: (rel) => nodeFs.readFileSync(join(dir, rel), "utf8"),
  };
}

const request = (d: { side: GitPatchSide; old: string; new: string }, hunks: GitPatchRequest["hunks"], path = "/long.txt"): GitPatchRequest => ({
  path,
  side: d.side,
  old: d.old,
  new: d.new,
  hunks,
});

/** A patch of `diff` with only the chosen lines, as `git add -p` writes an edited hunk
 *  (`reverse`: for applying backwards, where unchosen additions are context). */
function patchText(diff: { path: string; hunks: { old_start: number; new_start: number; lines: { kind: string; text: string }[] }[] }, choose: [number, number[] | null][], reverse: boolean): string {
  let out = `--- a/${diff.path}\n+++ b/${diff.path}\n`;
  for (const [h, lines] of [...choose].sort((a, z) => a[0] - z[0])) {
    const hunk = diff.hunks[h];
    const chosen = (i: number) => lines === null || lines.includes(i);
    let body = "";
    let old = 0;
    let now = 0;
    hunk.lines.forEach((line, i) => {
      if (line.kind === "context") {
        body += ` ${line.text}\n`;
        old++;
        now++;
      } else if (line.kind === "removed") {
        if (chosen(i)) {
          body += `-${line.text}\n`;
          old++;
        } else if (!reverse) {
          body += ` ${line.text}\n`;
          old++;
          now++;
        }
      } else if (chosen(i)) {
        body += `+${line.text}\n`;
        now++;
      } else if (reverse) {
        body += ` ${line.text}\n`;
        old++;
        now++;
      }
    });
    out += `@@ -${hunk.old_start},${old} +${hunk.new_start},${now} @@\n${body}`;
  }
  return out;
}

function gitApply(dir: string, patch: string, ...args: string[]) {
  execFileSync("git", ["-C", dir, "apply", ...args, "--unidiff-zero"], { input: patch, env: ENV });
}

function copyOf(f: Fixture): string {
  const to = nodeFs.mkdtempSync(join(tmpdir(), "rt-index-git-"));
  dirs.push(to);
  nodeFs.cpSync(f.dir, to, { recursive: true });
  return to;
}

const staged = (dir: string) => cli(dir, "ls-files", "-s");

describe.skipIf(!built || !hasGit)("the index of the web app", () => {
  it("stages chosen lines like `git apply --cached`", async () => {
    const f = fixture();
    f.write("long.txt", edited());
    const d = await f.api.indexDiff("/long.txt", "unstaged");
    expect(d.diff.hunks).toHaveLength(2);
    const added = d.diff.hunks[0].lines.flatMap((l, i) => (l.kind === "added" ? [i] : []));
    expect(added).toHaveLength(2);
    const copy = copyOf(f);
    gitApply(copy, patchText(d.diff, [[0, added]], false), "--cached");
    await f.api.stagePatch(request(d, [{ hunk: 0, lines: added }]));
    expect(staged(f.dir)).toBe(staged(copy));
    expect(cli(f.dir, "diff", "--cached", "--numstat")).toBe(cli(copy, "diff", "--cached", "--numstat"));
    // The file is untouched, and git sees the rest as unstaged.
    expect(f.read("long.txt")).toBe(edited());
    expect(cli(f.dir, "status", "--porcelain")).toBe("MM long.txt\n");
  });

  it("stages a whole hunk like `git add -p`", async () => {
    const f = fixture();
    f.write("long.txt", edited());
    const d = await f.api.indexDiff("/long.txt", "unstaged");
    const copy = copyOf(f);
    execFileSync("git", ["-C", copy, "add", "-p", "long.txt"], { input: "n\ny\n", env: ENV });
    await f.api.stagePatch(request(d, [{ hunk: 1 }]));
    expect(staged(f.dir)).toBe(staged(copy));
  });

  it("unstages chosen lines like `git apply --cached -R`, and everything back to HEAD", async () => {
    const f = fixture();
    f.write("long.txt", edited());
    cli(f.dir, "add", "long.txt");
    const d = await f.api.indexDiff("/long.txt", "staged");
    expect(d.diff.hunks).toHaveLength(2);
    const copy = copyOf(f);
    gitApply(copy, patchText(d.diff, [[1, null], [0, [1]]], true), "--cached", "-R");
    await f.api.unstagePatch(request(d, [{ hunk: 1 }, { hunk: 0, lines: [1] }]));
    expect(staged(f.dir)).toBe(staged(copy));
    const rest = await f.api.indexDiff("/long.txt", "staged");
    await f.api.unstagePatch(request(rest, rest.diff.hunks.map((_, hunk) => ({ hunk }))));
    expect(cli(f.dir, "diff", "--cached")).toBe("");
    expect(f.read("long.txt")).toBe(edited());
  });

  it("discards chosen lines like `git apply -R`, keeping the file as it was under a backup ref", async () => {
    const f = fixture();
    f.write("long.txt", edited());
    const d = await f.api.indexDiff("/long.txt", "unstaged");
    const copy = copyOf(f);
    gitApply(copy, patchText(d.diff, [[1, null], [0, [4]]], true), "-R");
    await f.api.discardPatch(request(d, [{ hunk: 1 }, { hunk: 0, lines: [4] }]));
    expect(f.read("long.txt")).toBe(nodeFs.readFileSync(join(copy, "long.txt"), "utf8"));
    expect(cli(f.dir, "diff", "--cached")).toBe("");
    const refs = cli(f.dir, "for-each-ref", "refs/requesttailor/backup");
    const line = refs.split("\n").find((l) => l.includes("/discard"));
    expect(line).toBeTruthy();
    const id = line!.split(/\s+/)[0];
    expect(cli(f.dir, "show", `${id}:long.txt`)).toBe(edited());
  });

  it("refuses a stale selection, the wrong side and lines that do not exist", async () => {
    const f = fixture();
    f.write("long.txt", edited());
    const d = await f.api.indexDiff("/long.txt", "unstaged");
    f.write("long.txt", `${edited()}eleven\n`);
    await expect(f.api.stagePatch(request(d, [{ hunk: 0 }]))).rejects.toThrow(/changed since/);
    await expect(f.api.discardPatch(request(d, [{ hunk: 0 }]))).rejects.toThrow(/changed since/);
    expect(cli(f.dir, "diff", "--cached")).toBe("");
    const fresh = await f.api.indexDiff("/long.txt", "unstaged");
    await expect(f.api.stagePatch({ ...request(fresh, [{ hunk: 0 }]), side: "staged" })).rejects.toThrow();
    await expect(f.api.stagePatch(request(fresh, [{ hunk: 9 }]))).rejects.toThrow();
    await expect(f.api.stagePatch(request(fresh, [{ hunk: 0, lines: [99] }]))).rejects.toThrow();
  });

  it("handles new and deleted files by lines", async () => {
    const f = fixture();
    f.write("new.txt", "a\nb\nc\n");
    const d = await f.api.indexDiff("/new.txt", "unstaged");
    expect(d.old).toBe("");
    await f.api.stagePatch(request(d, [{ hunk: 0, lines: [0, 1] }], "/new.txt"));
    expect(cli(f.dir, "show", ":new.txt")).toBe("a\nb\n");
    const s = await f.api.indexDiff("/new.txt", "staged");
    await f.api.unstagePatch(request(s, [{ hunk: 0 }], "/new.txt"));
    expect(cli(f.dir, "ls-files", "new.txt")).toBe("");
    expect(f.read("new.txt")).toBe("a\nb\nc\n");
    // A deleted file: all its lines stage the deletion.
    nodeFs.rmSync(join(f.dir, "notes.txt"));
    const gone = await f.api.indexDiff("/notes.txt", "unstaged");
    expect(gone.new).toBe("");
    await f.api.stagePatch(request(gone, [{ hunk: 0 }], "/notes.txt"));
    expect(cli(f.dir, "ls-files", "notes.txt")).toBe("");
    expect(cli(f.dir, "diff", "--cached", "--name-status")).toBe("D\tnotes.txt\n");
  });

  it("stages and unstages whole files like git", async () => {
    const f = fixture();
    f.write("notes.txt", "changed\n");
    f.write("added.txt", "new\n");
    nodeFs.rmSync(join(f.dir, "long.txt"));
    await f.api.stage(["/notes.txt", "/added.txt", "/long.txt"]);
    expect(cli(f.dir, "status", "--porcelain")).toBe("A  added.txt\nD  long.txt\nM  notes.txt\n");
    await f.api.unstage(["/added.txt", "/long.txt"]);
    expect(cli(f.dir, "status", "--porcelain")).toBe(" D long.txt\nM  notes.txt\n?? added.txt\n");
    await expect(f.api.stage(["/nothing.txt"])).rejects.toThrow(/neither in the working tree nor staged/);
  });

  it("commits the index only, refuses credentials and an empty index", async () => {
    const f = fixture();
    await expect(f.api.commitStaged({ message: "x" })).rejects.toThrow(/Nothing is staged/);
    f.write("notes.txt", "changed\n");
    f.write("other.txt", "unstaged\n");
    await f.api.stage(["/notes.txt"]);
    await expect(f.api.commitStaged({ message: "  " })).rejects.toThrow();
    const done = await f.api.commitStaged({ message: "Staged only" });
    expect(done.summary).toBe("Staged only");
    expect(cli(f.dir, "status", "--porcelain")).toBe("?? other.txt\n");
    expect(cli(f.dir, "show", "--stat", "--format=%s", "HEAD").trim()).toContain("notes.txt");
    // Credentials.
    f.write("keys.txt", "token = ghp_aBcDeFgHiJkLmNoPqRsTuVwXyZ0123456789\n");
    await f.api.stage(["/keys.txt"]);
    const head = cli(f.dir, "rev-parse", "HEAD");
    await expect(f.api.commitStaged({ message: "secrets" })).rejects.toMatchObject({ kind: "secrets-found" });
    expect(cli(f.dir, "rev-parse", "HEAD")).toBe(head);
  });

  it("verifies without a verifier and reads the repository's configuration", async () => {
    const f = fixture();
    const sig = "-----BEGIN SSH SIGNATURE-----\nabc\n-----END SSH SIGNATURE-----";
    expect(unverified("a", sig)).toEqual({ oid: "a", state: "unverified", kind: "ssh" });
    expect(unverified("a", "-----BEGIN PGP SIGNATURE-----\nx")).toMatchObject({ kind: "gpg" });
    expect(unverified("a", null)).toEqual({ oid: "a", state: "unsigned" });
    const [check] = await f.api.verify(["HEAD"]);
    expect(check.state).toBe("unsigned");
    // A commit that carries a gpgsig header.
    const head = cli(f.dir, "rev-parse", "HEAD").trim();
    const raw = cli(f.dir, "cat-file", "commit", head);
    const signed = raw.replace("\n\ninit", `\ngpgsig -----BEGIN SSH SIGNATURE-----\n abc\n -----END SSH SIGNATURE-----\n\ninit`);
    const oid = execFileSync("git", ["-C", f.dir, "hash-object", "-t", "commit", "-w", "--stdin", "--literally"], { input: signed, encoding: "utf8", env: ENV }).trim();
    const [s] = await f.api.verify([oid]);
    expect(s).toMatchObject({ state: "unverified", kind: "ssh" });
    expect(await f.api.config("user.name")).toBe("Ada");
    expect(await f.api.config("commit.template")).toBeNull();
  });
});
