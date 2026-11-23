// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Repositories in the web app's collection set (wave 15, strand A) over in-memory folder handles
// and the real core (rt-wasm, built with `node scripts/build-wasm.mjs`): a folder in Git without
// a collection is a repository root, repositories keep their order, the active repository
// follows the sidebar choice and the active collection, `repository()` (what the web Git opens)
// follows it, clone and init add folders, the summary. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import type { GitStatus } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { WebCollectionSet, type Core, type WebSession } from "./collections";
import { fill, memoryFolder } from "./git/fake-handle.test-util";
import { operationOf, webRepositories } from "./repositories";
import type { RepositoryFolders } from "./git";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

let loaded: Core | null = null;
async function core(): Promise<Core> {
  if (loaded) return loaded;
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as Core & {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  loaded = mod;
  return mod;
}

const manifest = (name: string) => `opencollection: "1.0.0"\ninfo:\n  name: ${name}\n`;
const GIT = { ".git/HEAD": "ref: refs/heads/main\n" };

async function folder(name: string, files: Record<string, string>) {
  const root = memoryFolder(name);
  await fill(root, files);
  return root;
}

function setFor(c: Core) {
  const remembered: { roots: string[]; active: string | null }[] = [];
  const opened: string[] = [];
  const set: WebCollectionSet = new WebCollectionSet({
    loadCore: async () => c,
    ensureReadWrite: async () => undefined,
    rememberFolder: async (handle) => `id-${handle.name}`,
    opened: (info) => opened.push(info.id),
    remember: (roots, active) => remembered.push({ roots, active }),
    session: (id, handle, ws): WebSession => ({ id, handle, ws, secrets: null }),
    pick: async () => null,
    saveActive: async () => [],
  });
  return { set, remembered, opened };
}

const status = (over: Partial<GitStatus> = {}): GitStatus => ({
  repo: true,
  branch: "main",
  upstream: "origin/main",
  ahead: 1,
  behind: 2,
  files: [{ path: "a.md", state: "modified" }],
  merging: false,
  ...over,
});

describe.skipIf(!built)("repositories in the web set", () => {
  it("a folder in Git without a collection is a repository root; outside Git it is not", async () => {
    const { set, remembered, opened } = setFor(await core());
    const notes = await folder("notes", { ...GIT, "README.md": "# notes\n" });
    const s = await set.addFolder(notes);
    expect(s.collections).toEqual([]);
    expect(s.active).toBeNull();
    expect(s.roots.map((r) => [r.id, r.collections])).toEqual([["id-notes", []]]);
    expect(s.repositories.map((r) => [r.id, r.name, r.collections])).toEqual([["id-notes", "notes", []]]);
    expect(s.active_repository).toBe("id-notes");
    // The web Git runs at the repository's folder, whole.
    expect(set.repository()).toEqual({ handle: notes, scope: "" });
    // Not in the recent list (that opens folders as collections); remembered with the set.
    expect(opened).toEqual([]);
    expect(remembered.at(-1)).toEqual({ roots: ["id-notes"], active: null });
    // Files are untouched: no scaffold.
    const names: string[] = [];
    for await (const e of notes.values()) names.push(e.name);
    expect(names.sort()).toEqual([".git", "README.md"]);
    // The same folder again changes nothing.
    expect((await set.addFolder(notes)).roots.length).toBe(1);
    // Outside Git, without a collection, the first folder is still a new collection (as before).
    const plain = setFor(await core()).set;
    await expect(plain.addFolder(await folder("plain", { "a.txt": "x" }))).rejects.toBeDefined();
  });

  it("lists repositories with collections first and follows the choice and the active collection", async () => {
    const { set } = setFor(await core());
    const work = await folder("work", { ...GIT, "api/opencollection.yml": manifest("API") });
    const notes = await folder("notes", { ...GIT, "todo.md": "- ship\n" });
    await set.openRoot(work);
    const s = await set.addFolder(notes);
    expect(s.repositories.map((r) => [r.id, r.collections.length])).toEqual([
      ["id-work", 1],
      ["id-notes", 0],
    ]);
    // The repository root was chosen when it joined.
    expect(s.active_repository).toBe("id-notes");
    expect(set.repository()).toEqual({ handle: notes, scope: "" });
    set.chooseRepository("id-work");
    expect(set.repository()?.handle).toBe(work);
    expect(set.repository()?.scope).toBe("");
    expect((await set.current()).active_repository).toBe("id-work");
    // A collection chosen in the sidebar brings Git back to it, scoped to its folder.
    set.chooseRepository("id-notes");
    await set.activate("id-work/api");
    expect((await set.current()).active_repository).toBe("id-work");
    expect(set.repository()?.scope).toBe("api");
    expect(() => set.chooseRepository("id-gone")).toThrow(/No repository/);
  });

  it("reloads a set that only has repository roots, and takes a repository out with its folder", async () => {
    const { set } = setFor(await core());
    await set.addFolder(await folder("notes", { ...GIT }));
    expect(await set.reloadSet()).toBeNull();
    expect((await set.current()).repositories.map((r) => r.id)).toEqual(["id-notes"]);
    expect(set.repositoryDirty()).toBe(false);
    const s = await set.removeRoot("id-notes");
    expect(s.repositories).toEqual([]);
    expect(s.active_repository).toBeUndefined();
  });

  it("brings a remembered repository root back", async () => {
    const c = await core();
    const work = await folder("work", { ...GIT, "opencollection.yml": manifest("W") });
    const notes = await folder("notes", { ...GIT });
    const { set } = setFor(c);
    const info = await set.restore("id-work", { roots: ["id-work", "id-notes"], active: "id-work" }, async (id) => (id === "id-work" ? work : notes));
    expect(info?.id).toBe("id-work");
    expect((await set.current()).repositories.map((r) => r.id)).toEqual(["id-work", "id-notes"]);
  });
});

describe.skipIf(!built)("web RepositoryApi", () => {
  function api(set: WebCollectionSet, over: Partial<RepositoryFolders> = {}, picked: FileSystemDirectoryHandle[] = []) {
    const calls: string[] = [];
    const folders: RepositoryFolders = {
      cloneFolder: async (url, name) => {
        calls.push(`clone ${url} ${name ?? ""}`);
        return folder(name ?? "copy", { ...GIT, "hello.md": "hi" });
      },
      initFolder: async (f) => {
        calls.push(`init ${f.name}`);
        await fill(f, GIT);
      },
      checkoutAt: async (_f, branch) => void calls.push(`checkout ${branch}`),
      statusAt: async () => status(),
      ...over,
    };
    let next = 0;
    return { calls, api: webRepositories({ collections: set, pick: async () => picked[next++] ?? null, folders, checkout: folders.checkoutAt }) };
  }

  it("clone adds the folder as a root, chosen; init does the same", async () => {
    const { set } = setFor(await core());
    const initial = await folder("first", { ...GIT });
    await set.addFolder(initial);
    const fresh = await folder("fresh", { "a.txt": "a" });
    const { api: repos, calls } = api(set, {}, [fresh]);

    const cloned = await repos.clone({ url: "https://git.example.com/acme/copy.git", name: "copy", branch: "dev" });
    expect(cloned?.id).toBe("id-copy");
    expect(calls).toEqual(["clone https://git.example.com/acme/copy.git copy", "checkout dev"]);
    expect((await set.current()).active_repository).toBe("id-copy");
    expect((await set.current()).repositories.map((r) => r.id)).toEqual(["id-first", "id-copy"]);

    const made = await repos.init();
    expect(made?.id).toBe("id-fresh");
    expect(calls.at(-1)).toBe("init fresh");
    expect((await set.current()).active_repository).toBe("id-fresh");
    // A cancelled picker adds nothing.
    expect(await repos.init()).toBeNull();
  });

  it("summary counts the whole work tree and reports a half-done operation", async () => {
    const { set } = setFor(await core());
    const notes = await folder("notes", { ...GIT });
    await set.addFolder(notes);
    const { api: repos } = api(set);
    expect(await repos.summary("id-notes")).toEqual({ id: "id-notes", branch: "main", upstream: "origin/main", ahead: 1, behind: 2, changes: 1 });
    await fill(notes, { ".git/CHERRY_PICK_HEAD": "abc\n" });
    expect((await repos.summary("id-notes")).operation).toBe("cherry-pick");
    await expect(repos.summary("id-gone")).rejects.toThrow(/No repository/);
    expect(await operationOf(notes, true)).toBe("cherry-pick");
    expect(await operationOf(await folder("calm", { ...GIT }), true)).toBe("merge");
    expect(await operationOf(await folder("calm2", { ...GIT }), false)).toBeUndefined();
    expect(await operationOf(await folder("rebasing", { ...GIT, ".git/rebase-merge/head-name": "x" }), false)).toBe("rebase");
  });

  it("activate chooses the repository", async () => {
    const { set } = setFor(await core());
    await set.openRoot(await folder("work", { ...GIT, "opencollection.yml": manifest("W") }));
    await set.addFolder(await folder("notes", { ...GIT }));
    const { api: repos } = api(set);
    expect((await repos.activate("id-work")).id).toBe("id-work");
    expect((await set.current()).active_repository).toBe("id-work");
    await expect(repos.activate("id-gone")).rejects.toBeDefined();
  });
});
