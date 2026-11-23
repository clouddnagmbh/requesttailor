// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ExtensionContext.suggest`, one test group per kind of suggested edit: the diff the user
// reviews, apply, undo, and a refusal when the target changed in between.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { RequestSpec, Script } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { folderScriptsOf, prepare, type FolderScriptsAccess } from "./apply";
import { diffRows, requestBlocks, scriptBlocks, totalStats } from "./model";

async function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  await app.select("users/list-users.yml");
  return { backend, app };
}

describe("request suggestions", () => {
  it("shows the changed fields, applies unsaved through applyExternal and undoes", async () => {
    const { backend, app } = await setup();
    const base = await backend.getRequest("users/list-users.yml");
    const next: RequestSpec = { ...base, url: base.url + "?limit=10", headers: [...base.headers, { name: "X-Id", value: "1", enabled: true }] };
    const p = await prepare(backend, app, { kind: "request", path: "users/list-users.yml", spec: next, note: { title: "Limit it" } });
    expect(p.blocks.map((b) => b.label)).toEqual(expect.arrayContaining(["URL", "Header › X-Id"]));
    expect(p.blocks.find((b) => b.label === "URL")).toMatchObject({ old: base.url, next: next.url });
    expect(p.note?.title).toBe("Limit it");
    expect(p.undoable).toBe(true);

    const applied = await p.apply();
    expect((await backend.getRequest("users/list-users.yml")).url).toBe(next.url);
    expect(app.drafts["users/list-users.yml"]?.url).toBe(next.url); // the open tab shows it
    expect(await backend.dirty()).toContain("users/list-users.yml");

    expect(await applied.undo!()).toBe(true);
    expect((await backend.getRequest("users/list-users.yml")).url).toBe(base.url);
    expect(app.drafts["users/list-users.yml"]?.url).toBe(base.url);
  });

  it("refuses to apply when the request changed after the suggestion was prepared", async () => {
    const { backend, app } = await setup();
    const base = await backend.getRequest("users/list-users.yml");
    const p = await prepare(backend, app, { kind: "request", path: "users/list-users.yml", spec: { ...base, url: "https://x.test/a" } });
    await app.edit("users/list-users.yml", { ...base, url: "https://typed.test/" });
    await expect(p.apply()).rejects.toMatchObject({ kind: "stale" });
    expect((await backend.getRequest("users/list-users.yml")).url).toBe("https://typed.test/");
  });

  it("does not undo over a later edit", async () => {
    const { backend, app } = await setup();
    const base = await backend.getRequest("users/list-users.yml");
    const applied = await (await prepare(backend, app, { kind: "request", path: "users/list-users.yml", spec: { ...base, url: "https://x.test/a" } })).apply();
    await app.edit("users/list-users.yml", { ...base, url: "https://later.test/" });
    expect(await applied.undo!()).toBe(false);
    expect((await backend.getRequest("users/list-users.yml")).url).toBe("https://later.test/");
  });

  it("a request that does not exist is a clear error", async () => {
    const { backend, app } = await setup();
    const base = await backend.getRequest("users/list-users.yml");
    await expect(prepare(backend, app, { kind: "request", path: "nope.yml", spec: base })).rejects.toMatchObject({ kind: "not-found" });
  });
});

describe("new request suggestions", () => {
  it("lists what the new request has, creates it unsaved and opens it", async () => {
    const { backend, app } = await setup();
    const spec: RequestSpec = { ...(await backend.getRequest("users/list-users.yml")), name: "Health check", url: "https://x.test/health" };
    const p = await prepare(backend, app, { kind: "new-request", folder: "users", spec });
    expect(p.blocks.some((b) => b.label === "URL" && b.old === null && b.next === "https://x.test/health")).toBe(true);
    expect(p.undoable).toBe(false);
    const before = (await backend.tree()) && app.openTabs.length;
    const applied = await p.apply();
    expect(applied.undo).toBeNull();
    expect(app.openTabs.length).toBe(before + 1);
    expect(app.drafts[app.active!]?.url).toBe("https://x.test/health");
  });
});

describe("folder script suggestions", () => {
  const scripts = (code: string): Script[] => [{ phase: "before-request", code }];

  it("is refused where the backend cannot change folder scripts", async () => {
    const { backend, app } = await setup();
    expect(folderScriptsOf(backend)).toBeNull();
    await expect(prepare(backend, app, { kind: "folder-scripts", path: "users", scripts: scripts("a") })).rejects.toMatchObject({ kind: "unsupported" });
  });

  it("applies and undoes through the folder script access, and refuses a stale folder", async () => {
    const { backend, app } = await setup();
    let stored: Script[] = scripts("rt.vars.set('a', 1);\n");
    const access: FolderScriptsAccess = { get: async () => stored, set: async (_p, s) => void (stored = s) };
    (backend as unknown as { folderScripts: FolderScriptsAccess }).folderScripts = access;

    const p = await prepare(backend, app, { kind: "folder-scripts", path: "users", scripts: scripts("rt.vars.set('a', 2);\n") });
    expect(p.blocks).toHaveLength(1);
    expect(p.blocks[0].label).toBe("Script › Before the request");
    const applied = await p.apply();
    expect(stored[0].code).toContain("2");
    expect(await applied.undo!()).toBe(true);
    expect(stored[0].code).toContain("1");

    const p2 = await prepare(backend, app, { kind: "folder-scripts", path: "users", scripts: scripts("x") });
    stored = scripts("someone else");
    await expect(p2.apply()).rejects.toMatchObject({ kind: "stale" });
  });
});

describe("file suggestions", () => {
  it("stages the text, shows the line diff and undoes", async () => {
    const { backend, app } = await setup();
    await backend.companions.stage("CHANGELOG.md", "# Changes\n- one\n");
    const p = await prepare(backend, app, { kind: "file", path: "CHANGELOG.md", text: "# Changes\n- one\n- two\n" });
    expect(totalStats(p.blocks)).toEqual({ added: 1, removed: 0 });
    const applied = await p.apply();
    expect(await backend.companions.read("CHANGELOG.md")).toBe("# Changes\n- one\n- two\n");
    expect(await applied.undo!()).toBe(true);
    expect(await backend.companions.read("CHANGELOG.md")).toBe("# Changes\n- one\n");
  });

  it("a new file has no undo; a changed file is refused; yml is refused up front", async () => {
    const { backend, app } = await setup();
    const p = await prepare(backend, app, { kind: "file", path: "docs/NOTES.md", text: "hello\n" });
    expect(p.title).toBe("New file");
    expect((await p.apply()).undo).toBeNull();
    const q = await prepare(backend, app, { kind: "file", path: "docs/NOTES.md", text: "bye\n" });
    await backend.companions.stage("docs/NOTES.md", "typed meanwhile\n");
    await expect(q.apply()).rejects.toMatchObject({ kind: "stale" });
    await expect(prepare(backend, app, { kind: "file", path: "a/b.yml", text: "" })).rejects.toMatchObject({ kind: "invalid" });
  });
});

describe("blocks and rows", () => {
  it("names headers by their name, so a reordered list is no change", () => {
    const base = { name: "a", method: "GET", url: "u", headers: [{ name: "A", value: "1", enabled: true }, { name: "B", value: "2", enabled: true }] } as unknown as RequestSpec;
    const swapped = { ...base, headers: [base.headers[1], base.headers[0]] };
    expect(requestBlocks(base, swapped)).toEqual([]);
  });

  it("collapses long unchanged stretches into one gap row", () => {
    const old = Array.from({ length: 30 }, (_, i) => `line ${i}`).join("\n");
    const next = old.replace("line 15", "line fifteen");
    const rows = diffRows(old, next, 2);
    expect(rows.filter((r) => r.kind === "gap").map((r) => r.hidden)).toEqual([13, 12]);
    expect(rows.filter((r) => r.kind === "added")).toHaveLength(1);
    expect(rows.find((r) => r.kind === "added")).toMatchObject({ newLine: 16, oldLine: null });
  });

  it("scriptBlocks only lists phases that change", () => {
    const b = scriptBlocks([{ phase: "tests", code: "a" }], [{ phase: "tests", code: "a" }, { phase: "before-request", code: "b" }]);
    expect(b.map((x) => x.label)).toEqual(["Script › Before the request"]);
  });
});
