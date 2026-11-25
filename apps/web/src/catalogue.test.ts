// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The plugin catalogue over the real core (rt-wasm, built with `node scripts/build-wasm.mjs`)
// against a local fake index host (Node's fetch reaches it): sources are only stored until a
// refresh, the checksum is verified before an install (a wrong one, a tampered module and a
// republished release refuse), the rights are the module's own, an update with more rights is
// flagged, and the origin is kept. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { createHash } from "node:crypto";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { INSTALLED_KEY, SOURCES_KEY, webCatalogue, type CatalogueStorage } from "./catalogue";
import { memoryPluginStore, webPlugins, type BundledSource } from "./plugins";
import type { WebHost } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const FIXTURES = join(__dirname, "../../../crates/rt-plugin/tests/fixtures");

const V1 = nodeFs.readFileSync(join(FIXTURES, "demo.wasm"));
const V2 = nodeFs.readFileSync(join(FIXTURES, "demo-v2.wasm"));
const sha = (b: Buffer) => createHash("sha256").update(b).digest("hex");
const tampered = (b: Buffer) => Buffer.concat([b, Buffer.from([0x00, 0x02, 0x01, 0x78])]);

interface Release {
  version: string;
  wasm: Buffer;
  listed?: Buffer;
  claimed?: unknown;
}

let server: Server;
let base = "";
let release: Release = { version: "1.0.0", wasm: V1, claimed: { storage: true } };
const asked: string[] = [];

beforeAll(async () => {
  server = createServer((req, res) => {
    asked.push(req.url ?? "");
    const send = (status: number, body: Buffer | string) => {
      res.writeHead(status);
      res.end(body);
    };
    if (req.url === "/plugins.json") {
      const listed = release.listed ?? release.wasm;
      return send(
        200,
        JSON.stringify({
          version: 1,
          name: "Fake index",
          plugins: [
            {
              id: "demo",
              name: "Demo plugin",
              version: release.version,
              api: 1,
              description: "Synthetic",
              wasm: { url: "demo.wasm", sha256: sha(listed), size: listed.length },
              permissions: release.claimed ?? {},
            },
          ],
        }),
      );
    }
    if (req.url === "/demo.wasm") return send(200, release.wasm);
    send(404, "not found");
  });
  await new Promise<void>((r) => server.listen(0, "127.0.0.1", r));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
  server?.close();
});

async function setup() {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as { initSync(o: { module: Uint8Array }): unknown };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  // The core's plugin host is one per page (here: per test file): start each test without `demo`.
  try {
    (mod as unknown as { Plugins: { unload(id: string): void } }).Plugins.unload("demo");
  } catch {
    // Not loaded yet.
  }
  const host = {
    deps: { loadCore: async () => mod },
    companionCredentials: async () => null,
  } as unknown as WebHost;
  const bundled: BundledSource = {
    ids: [],
    fetch: async () => null,
    manifest: async () => null,
    load: () => ({ done: true, bundled: {} }),
    save: () => {},
  };
  const plugins = webPlugins(host, async () => memoryPluginStore(), bundled);
  const memory = new Map<string, string>();
  const storage: CatalogueStorage = { read: (k) => memory.get(k) ?? null, write: (k, v) => void memory.set(k, v) };
  return { catalogue: webCatalogue(host, () => false, storage), plugins, memory };
}

describe.skipIf(!built)("web plugin catalogue (real core)", () => {
  it("keeps sources without asking the network, then lists the index on refresh", async () => {
    release = { version: "1.0.0", wasm: V1, claimed: { storage: true } };
    const { catalogue, memory } = await setup();
    const before = asked.length;
    expect(await catalogue.sources()).toEqual([]);
    await catalogue.addSource(`${base}/plugins.json`);
    expect(JSON.parse(memory.get(SOURCES_KEY)!)).toHaveLength(1);
    expect(asked.length).toBe(before);
    await expect(catalogue.addSource("http://evil.example.test/plugins.json")).rejects.toThrow(/https/);
    const [got] = await catalogue.refresh();
    expect(got?.index.plugins[0]?.id).toBe("demo");
    expect(got?.source.name).toBe("Fake index");
    expect(asked.length).toBe(before + 1);
    expect((await catalogue.sources())[0]?.name).toBe("Fake index");
  });

  it("takes the rights from the module and installs it with its origin", async () => {
    release = { version: "1.0.0", wasm: V1, claimed: { network: ["*"] } };
    const { catalogue, plugins, memory } = await setup();
    const url = `${base}/plugins.json`;
    await catalogue.addSource(url);
    const plan = await catalogue.plan(url, "demo");
    expect(plan.sha256).toBe(sha(V1));
    expect(plan.permissions).toEqual({ network: [], storage: true, variables: false });
    expect(plan.permissions_differ).toBe(true);
    expect(plan.warnings.join(" ")).toMatch(/other rights/);
    expect(plan.update).toBe(false);
    const got = await catalogue.install(plan);
    expect(got).toMatchObject({ plugin: "demo", version: "1.0.0", source: url, sha256: sha(V1) });
    expect((await plugins.list()).map((p) => p.manifest.id)).toEqual(["demo"]);
    expect(JSON.parse(memory.get(INSTALLED_KEY)!).demo.sha256).toBe(sha(V1));
    expect(await catalogue.installed()).toHaveLength(1);
  });

  it("refuses a wrong checksum before anything is installed", async () => {
    release = { version: "1.0.0", wasm: tampered(V1), listed: V1 };
    const { catalogue, plugins } = await setup();
    const url = `${base}/plugins.json`;
    await catalogue.addSource(url);
    await expect(catalogue.plan(url, "demo")).rejects.toThrow(/checksum does not match/);
    expect(await plugins.list()).toEqual([]);
  });

  it("refuses a republished release with the same version, and flags an update with more rights", async () => {
    release = { version: "1.0.0", wasm: V1, claimed: { storage: true } };
    const { catalogue, plugins } = await setup();
    const url = `${base}/plugins.json`;
    await catalogue.addSource(url);
    await catalogue.install(await catalogue.plan(url, "demo"));

    release = { version: "1.0.0", wasm: tampered(V1), claimed: { storage: true } };
    await expect(catalogue.plan(url, "demo")).rejects.toThrow(/the index changed a published release/);
    expect((await plugins.list())[0]?.manifest.version).toBe("1.0.0");

    release = { version: "2.0.0", wasm: V2, claimed: { network: ["{request}"], storage: true } };
    const updates = await catalogue.checkUpdates();
    expect(updates).toEqual([{ plugin: "demo", source: url, installed: "1.0.0", available: "2.0.0" }]);
    const plan = await catalogue.plan(url, "demo");
    expect(plan.update).toBe(true);
    expect(plan.more_permissions).toBe(true);
    expect(plan.permissions.network).toEqual(["{request}"]);
    await catalogue.install(plan);
    expect((await plugins.list())[0]?.manifest.version).toBe("2.0.0");
  });

  it("does not install a module that changed between the plan and the install", async () => {
    release = { version: "1.0.0", wasm: V1, claimed: { storage: true } };
    const { catalogue, plugins } = await setup();
    const url = `${base}/plugins.json`;
    await catalogue.addSource(url);
    const plan = await catalogue.plan(url, "demo");
    release = { version: "1.0.0", wasm: tampered(V1), claimed: { storage: true } };
    await expect(catalogue.install(plan)).rejects.toThrow(/checksum does not match|changed/);
    expect(await plugins.list()).toEqual([]);
  });
});
