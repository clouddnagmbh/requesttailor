// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's collection set (wave 14, strand A) over in-memory folder handles and the real
// core (rt-wasm, built with `node scripts/build-wasm.mjs`): discovery through the core's rule,
// ids per root, collections read on demand, adding and removing folders, save all, search over
// all, new collections. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import type { FileChange } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { WebCollectionSet, type Core, type WebSession } from "./collections";
import { writeChanges } from "./fs/directory";
import { fill, memoryFolder } from "./git/fake-handle.test-util";

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
const request = (name: string, url: string) =>
  `info:\n  name: ${name}\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "${url}"\n`;

/** Billing, Orders and Orders' nested Legacy, plus decoys discovery must never enter. */
async function sampleRoot(name = "apis"): Promise<FileSystemDirectoryHandle> {
  const root = memoryFolder(name);
  await fill(root, {
    "billing/opencollection.yml": manifest("Billing"),
    "billing/ping.yml": request("Ping billing", "http://billing/ping"),
    "services/orders/opencollection.yml": manifest("Orders"),
    "services/orders/ping.yml": request("Ping orders", "http://orders/ping"),
    "services/orders/legacy/opencollection.yml": manifest("Legacy orders"),
    "services/orders/legacy/ping.yml": request("Ping legacy", "http://legacy/ping"),
    "node_modules/lib/opencollection.yml": manifest("Decoy"),
    ".git/HEAD": "ref: refs/heads/main\n",
    "target/x/opencollection.yml": manifest("Decoy"),
    "README.md": "several collections\n",
  });
  return root;
}

function setFor(c: Core, picks: FileSystemDirectoryHandle[] = []) {
  let next = 0;
  const remembered: { roots: string[]; active: string | null }[] = [];
  const opened: string[] = [];
  const set: WebCollectionSet = new WebCollectionSet({
    loadCore: async () => c,
    ensureReadWrite: async () => undefined,
    rememberFolder: async (handle) => `id-${handle.name}`,
    opened: (info) => opened.push(info.id),
    remember: (roots, active) => remembered.push({ roots, active }),
    session: (id, handle, ws): WebSession => ({ id, handle, ws, secrets: null }),
    pick: async () => picks[next++] ?? null,
    saveActive: async () => {
      const s = set.session();
      const prepared = s.ws.prepareSave();
      try {
        const changes: FileChange[] = [...prepared.changes];
        for (const f of s.created ?? []) if (!changes.some((x) => x.path === f.path)) changes.push(f);
        await writeChanges(s.handle, changes);
        s.ws.commit(prepared);
        s.created = undefined;
        return changes;
      } finally {
        prepared.free();
      }
    },
  });
  return { set, remembered, opened };
}

async function text(root: FileSystemDirectoryHandle, path: string): Promise<string> {
  const parts = path.split("/");
  let dir = root;
  for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p);
  return (await (await dir.getFileHandle(parts[parts.length - 1])).getFile()).text();
}

describe.skipIf(!built)("web collection set", () => {
  it("finds three collections with the core's rule and opens the first", async () => {
    const c = await core();
    const { set, remembered, opened } = setFor(c);
    const info = await set.openRoot(await sampleRoot());
    const s = await set.current();
    expect(s.collections.map((x) => [x.id, x.name, x.dir])).toEqual([
      ["id-apis/billing", "Billing", "billing"],
      ["id-apis/services/orders", "Orders", "services/orders"],
      ["id-apis/services/orders/legacy", "Legacy orders", "services/orders/legacy"],
    ]);
    expect(info?.id).toBe("id-apis/billing");
    expect(s.active).toBe("id-apis/billing");
    expect(s.roots.map((r) => [r.id, r.collections.length, r.truncated])).toEqual([["id-apis", 3, false]]);
    // `.git` in the root: every collection is in that repository.
    expect(s.collections.every((x) => x.repo === "id-apis")).toBe(true);
    // The web Git runs at the repository's folder, scoped to the collection's folder.
    expect(set.repository()?.scope).toBe("billing");
    await set.activate("id-apis/services/orders");
    expect(set.repository()?.scope).toBe("services/orders");
    await set.activate("id-apis/billing");
    expect(opened).toEqual(["id-apis"]);
    expect(remembered.at(-1)).toEqual({ roots: ["id-apis"], active: "id-apis/billing" });
    // The nested collection is not part of Orders.
    const orders = await set.tree("id-apis/services/orders");
    expect(orders.kind === "folder" && orders.children.map((n) => n.path)).toEqual(["ping.yml"]);
  });

  it("a root that is a collection keeps the root's id", async () => {
    const c = await core();
    const root = memoryFolder("single");
    await fill(root, { "opencollection.yml": manifest("Single"), "a.yml": request("A", "http://a") });
    const { set } = setFor(c);
    const info = await set.openRoot(root);
    expect(info).toEqual({ id: "id-single", name: "Single", location: "single" });
  });

  it("adds a folder once, refuses one without collections, removes it when saved", async () => {
    const c = await core();
    const other = memoryFolder("payments");
    await fill(other, { "opencollection.yml": manifest("Payments"), "ping.yml": request("Ping", "http://p") });
    const empty = memoryFolder("empty");
    const apis = await sampleRoot();
    const { set } = setFor(c, [other, other, empty]);
    await set.openRoot(apis);
    expect((await set.add())?.collections.map((x) => x.name)).toEqual(["Billing", "Orders", "Legacy orders", "Payments"]);
    expect((await set.current()).active).toBe("id-payments");
    expect((await set.add())?.collections).toHaveLength(4);
    await expect(set.add()).rejects.toThrow(/no collection in "empty"/);

    const s = set.session();
    const spec = s.ws.request("ping.yml");
    s.ws.updateRequest("ping.yml", { ...spec, url: "http://changed" });
    await expect(set.removeRoot("id-payments")).rejects.toThrow(/unsaved changes/);
    await set.saveAll();
    const after = await set.removeRoot("id-payments");
    expect(after.collections).toHaveLength(3);
    expect(after.active).toBe("id-apis/billing");
  });

  it("saves every collection with changes, the same path in two of them", async () => {
    const c = await core();
    const apis = await sampleRoot();
    const { set } = setFor(c);
    await set.openRoot(apis);
    const edit = async (id: string, url: string) => {
      await set.activate(id);
      const s = set.session();
      s.ws.updateRequest("ping.yml", { ...s.ws.request("ping.yml"), url });
    };
    await edit("id-apis/billing", "http://billing.example/ping");
    await edit("id-apis/services/orders", "http://orders.example/ping");
    expect(await set.dirty("id-apis/billing")).toEqual(["ping.yml"]);
    const saved = await set.saveAll();
    expect(saved.map((x) => x.collection)).toEqual(["id-apis/billing", "id-apis/services/orders"]);
    expect(await text(apis, "billing/ping.yml")).toContain("billing.example");
    expect(await text(apis, "services/orders/ping.yml")).toContain("orders.example");
    expect(set.session().id).toBe("id-apis/services/orders");
  });

  it("searches every collection, names first", async () => {
    const c = await core();
    const { set } = setFor(c);
    await set.openRoot(await sampleRoot());
    const hits = await set.search("ping", 20);
    expect(new Set(hits.map((h) => h.collection)).size).toBe(3);
    expect(hits.slice(0, 3).every((h) => h.field === "name")).toBe(true);
  });

  it("creates a collection whose files are written on save", async () => {
    const c = await core();
    const apis = await sampleRoot();
    const { set } = setFor(c);
    await set.openRoot(apis);
    const info = await set.create({ name: "Inventory", root: "id-apis", dir: "inventory", base_url: "http://localhost:8080", environment: "Local" });
    expect(info?.id).toBe("id-apis/inventory");
    expect(set.session().ws.environments().map((e) => e.name)).toEqual(["Local"]);
    expect((await set.dirty("id-apis/inventory")).length).toBeGreaterThan(0);
    await set.saveAll();
    expect(await text(apis, "inventory/opencollection.yml")).toContain("Inventory");
    expect(await text(apis, "inventory/environments/Local.yml")).toContain("localhost:8080");
    await expect(
      set.create({ name: "X", root: "id-apis", dir: "billing", base_url: null, environment: null }),
    ).rejects.toThrow(/already/);
    await expect(set.create({ name: "X", root: "id-apis", dir: "../out", base_url: null, environment: null })).rejects.toThrow();
  });

  it("restores the remembered folders with their active collection", async () => {
    const c = await core();
    const apis = await sampleRoot();
    const other = memoryFolder("payments");
    await fill(other, { "opencollection.yml": manifest("Payments") });
    const { set } = setFor(c);
    const handles: Record<string, FileSystemDirectoryHandle> = { "id-apis": apis, "id-payments": other };
    const info = await set.restore(
      "id-payments",
      { roots: ["id-apis", "id-payments"], active: "id-apis/services/orders" },
      async (id) => handles[id] ?? null,
    );
    expect(info?.id).toBe("id-apis/services/orders");
    expect((await set.current()).collections).toHaveLength(4);
    expect(await set.restore("id-elsewhere", { roots: ["id-apis"], active: null }, async () => null)).toBeNull();
  });
});
