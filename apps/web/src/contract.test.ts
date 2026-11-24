// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's contract API (wave 12, strand F) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`) and a real Git repository on Node's fs: the same fixture and
// the same answers as crates/rt-format/tests/contract.rs and `rt changelog`. Skipped when the
// core is not built.

import * as nodeFs from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { afterEach, describe, expect, it } from "vitest";

import type { GitApi } from "@requesttailor/ui";

import { webContract } from "./contract";
import { fill, memoryFolder } from "./git/fake-handle.test-util";
import { writeChanges } from "./fs/directory";
import type { FsClient } from "./git/fs";
import * as repoOps from "./git/repo";
import type { Repo } from "./git/repo";
import * as rev from "./git/revisions";
import type { Wave12Host } from "./wave12";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const FIXTURES = join(__dirname, "../../../testdata/contract");
const me = { name: "Test User", email: "test@example.com" };

const dirs: string[] = [];
afterEach(() => {
  for (const d of dirs.splice(0)) nodeFs.rmSync(d, { recursive: true, force: true });
});

async function core() {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod;
}

function collectionFiles(): Record<string, string> {
  const out: Record<string, string> = {};
  const walk = (dir: string, prefix: string) => {
    for (const e of nodeFs.readdirSync(dir, { withFileTypes: true })) {
      if (e.isDirectory()) walk(join(dir, e.name), `${prefix}${e.name}/`);
      else out[`${prefix}${e.name}`] = nodeFs.readFileSync(join(dir, e.name), "utf8");
    }
  };
  walk(join(FIXTURES, "collection"), "");
  // No configured spec: the candidates apply.
  out["opencollection.yml"] = "opencollection: 1.0.0\ninfo:\n  name: Orders\n";
  return out;
}

async function repoWith(specs: string[], messages: string[] = []): Promise<Repo> {
  const dir = nodeFs.mkdtempSync(join(tmpdir(), "rt-contract-"));
  dirs.push(dir);
  const r: Repo = { fs: nodeFs as unknown as FsClient, dir, cache: {} };
  await repoOps.init(r);
  const files = collectionFiles();
  for (const [p, t] of Object.entries(files)) {
    nodeFs.mkdirSync(join(dir, p, ".."), { recursive: true });
    nodeFs.writeFileSync(join(dir, p), t);
  }
  for (const [i, spec] of specs.entries()) {
    nodeFs.writeFileSync(join(dir, "openapi.yaml"), nodeFs.readFileSync(join(FIXTURES, "specs", spec)));
    const paths = i === 0 ? [...Object.keys(files), "openapi.yaml"] : ["openapi.yaml"];
    await repoOps.commit(r, messages[i] ?? `spec ${i + 1}`, paths, me);
  }
  return r;
}

function api(r: Repo, loaded: Awaited<ReturnType<typeof core>>) {
  const git = {
    treeAt: (rv: string) => rev.treeAt(r, rv),
    show: (rv: string, p: string) => rev.show(r, rv, p),
    commits: (from: string | null, to: string, limit: number) => rev.commits(r, from, to, limit),
    changelogCommits: (from: string, to: string) => rev.changelogCommits(r, from, to),
  } as unknown as GitApi;
  const host = { deps: { loadCore: async () => loaded } } as unknown as Wave12Host["host"];
  return webContract({ host, git, sendContext: async () => Promise.reject(new Error("unused")) });
}

describe.skipIf(!built)("web contract API over the real core", () => {
  it("checks the collection at a revision", async () => {
    const c = api(await repoWith(["v1.yaml", "v3.yaml"]), await core());
    const report = await c.check("HEAD~1");
    expect(report.findings).toEqual([]);
    expect([report.operations, report.covered]).toEqual([5, 5]);
    // Against v3 the delete request lost its operation.
    const now = await c.check("HEAD");
    expect(now.findings.map((f) => [f.request, f.kind])).toContainEqual(["orders/delete-order.yml", "no-operation"]);
  });

  it("lists breaking changes between revisions with their requests", async () => {
    const c = api(await repoWith(["v1.yaml", "v3.yaml"]), await core());
    const changes = await c.changes("HEAD~1", "HEAD");
    expect([changes.from_version, changes.to_version]).toEqual(["1.0.0", "2.0.0"]);
    const removed = changes.changes.find((x) => x.kind === "operation-removed");
    expect(removed).toMatchObject({ breaking: true, operation: "DELETE /orders/{id}", requests: ["orders/delete-order.yml"] });
    expect(changes.changes.filter((x) => x.breaking).length).toBeGreaterThanOrEqual(5);
  });

  it("writes the same changelog as rt changelog", async () => {
    const c = api(await repoWith(["v1.yaml", "v3.yaml"]), await core());
    const text = await c.changelog("HEAD~1", "HEAD");
    expect(text.startsWith("# Orders API 1.0.0 → 2.0.0\n")).toBe(true);
    expect(text).toContain("## Breaking changes");
    expect(text).toContain("`DELETE /orders/{id}`");
    expect(text).toContain("`orders/delete-order.yml`");
  });

  it("hands writers the same facts as rt changelog: the changelog, its parts and the commits", async () => {
    const r = await repoWith(["v1.yaml", "v3.yaml"], ["v1", "Merge pull request #12 from acme/x\n\nDrops delete"]);
    const c = api(r, await core());
    const facts = await c.changelogFacts("HEAD~1", "HEAD");
    expect([facts.from, facts.to]).toEqual(["HEAD~1", "HEAD"]);
    expect(facts.markdown).toBe(await c.changelog("HEAD~1", "HEAD"));
    expect(facts.title).toBe("Orders API 1.0.0 → 2.0.0");
    expect(facts.spec.changes.filter((x) => x.breaking).length).toBeGreaterThanOrEqual(5);
    expect(facts.commits).toEqual([
      expect.objectContaining({
        summary: "Merge pull request #12 from acme/x",
        body: "Drops delete",
        author: "Test User",
        pull: 12,
      }),
    ]);
    expect(facts.commits?.[0].oid).toMatch(/^[0-9a-f]{40}$/);
  });

  it("refuses to check without a spec", async () => {
    const r = await repoWith(["v1.yaml"]);
    nodeFs.rmSync(join(r.dir, "openapi.yaml"));
    await repoOps.commit(r, "drop", ["openapi.yaml"], me);
    const c = api(r, await core());
    await expect(c.check("HEAD")).rejects.toMatchObject({ kind: "format" });
    expect((await c.changes("HEAD~1", "HEAD")).changes.every((x) => x.kind === "operation-removed")).toBe(true);
  });
});

describe.skipIf(!built)("stageSpec over the real core", () => {
  const SPEC = nodeFs.readFileSync(join(FIXTURES, "specs", "v1.yaml"), "utf8");
  const NEWER = SPEC.replace(/version: .*\n/, "version: 9.9.9\n");

  async function staged(files: Record<string, string>) {
    const loaded = await core();
    const handle = memoryFolder();
    await fill(handle, files);
    const ws = (loaded as unknown as { Workspace: { open(f: Map<string, string>): unknown } }).Workspace.open(
      new Map(Object.entries(files)),
    ) as {
      prepareSave(): { changes: { path: string; text: string; deleted: boolean }[]; free(): void };
      commit(p: unknown): void;
      dirty(): string[];
    };
    const host = { deps: { loadCore: async () => loaded }, session: () => ({ handle, ws }) } as unknown as Wave12Host["host"];
    const c = webContract({ host, git: {} as GitApi, sendContext: async () => Promise.reject(new Error("unused")) });
    return { c, handle, ws };
  }

  it("stages the located spec unsaved and a save writes it", async () => {
    const { c, handle, ws } = await staged({
      "opencollection.yml": "opencollection: 1.0.0\ninfo:\n  name: Orders\n",
      "openapi.yaml": SPEC,
    });
    expect(await c.stageSpec(NEWER)).toBe("openapi.yaml");
    expect(ws.dirty()).toEqual(["openapi.yaml"]);
    const prepared = ws.prepareSave();
    try {
      expect(prepared.changes.map((x) => x.path)).toEqual(["openapi.yaml"]);
      await writeChanges(handle, [...prepared.changes]);
      ws.commit(prepared);
    } finally {
      prepared.free();
    }
    const file = await (await handle.getFileHandle("openapi.yaml")).getFile();
    expect(await file.text()).toBe(NEWER);
    expect(ws.dirty()).toEqual([]);
  });

  it("refuses a text that is no spec and a collection without one", async () => {
    const { c, ws } = await staged({
      "opencollection.yml": "opencollection: 1.0.0\ninfo:\n  name: Orders\n",
      "openapi.yaml": SPEC,
    });
    await expect(c.stageSpec("info:\n  name: x\nhttp:\n  method: GET\n  url: http://x\n")).rejects.toMatchObject({ kind: "format" });
    expect(ws.dirty()).toEqual([]);
    const none = await staged({ "opencollection.yml": "opencollection: 1.0.0\ninfo:\n  name: Orders\n" });
    await expect(none.c.stageSpec(NEWER)).rejects.toMatchObject({ kind: "format" });
  });
});
