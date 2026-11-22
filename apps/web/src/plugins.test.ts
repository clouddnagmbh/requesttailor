// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web plugin part against a fake core: what is stored, what the core is told, what a
// send gets. The real host (rt-wasm) is covered by rt-plugin's tests and the shell e2e test.

import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

import type { Manifest, Permissions, PluginInfo } from "@requesttailor/ui";

import {
  embeddedManifest,
  memoryPluginStore,
  newer,
  pluginsOf,
  webPlugins,
  type BundledSource,
  type CorePlugins,
  type PluginServices,
  type PluginStore,
  type WelcomeRecord,
} from "./plugins";
import type { WebHost } from "./web-backend";

const NONE: Permissions = { network: [], storage: false, variables: false };

/** A `.wasm` stand-in: the bytes are `<id>@<version>`. */
function file(id: string, version = "1.0.0") {
  return { name: `${id}.wasm`, data: new TextEncoder().encode(`${id}@${version}`) };
}

function fakeCore() {
  const loaded: { id: string; version: string; granted: Permissions; enabled: boolean }[] = [];
  const calls: string[] = [];
  let lastServices: PluginServices | null = null;
  let lastTab: unknown[] = [];
  const manifest = (wasm: Uint8Array): Manifest => {
    const [id, version] = new TextDecoder().decode(wasm).split("@");
    return {
      api: 1,
      id: id!,
      name: id!,
      version: version!,
      description: "",
      license: null,
      homepage: null,
      permissions: { ...NONE, storage: true },
      contributes: { prepare: false, response: false, tabs: [], importers: [], welcome: null },
    };
  };
  const info = (p: (typeof loaded)[number]): PluginInfo => ({
    manifest: { ...manifest(new TextEncoder().encode(`${p.id}@${p.version}`)) },
    granted: true,
    enabled: p.enabled,
  });
  const plugins: CorePlugins = {
    inspect: manifest,
    load: (wasm, granted, enabled) => {
      const m = manifest(wasm);
      calls.push(`load ${m.id}`);
      const entry = { id: m.id, version: m.version, granted, enabled };
      const at = loaded.findIndex((p) => p.id === m.id);
      if (at >= 0) loaded[at] = entry;
      else loaded.push(entry);
      return info(entry);
    },
    unload: (id) => {
      calls.push(`unload ${id}`);
      loaded.splice(
        loaded.findIndex((p) => p.id === id),
        1,
      );
    },
    setEnabled: (id, enabled) => {
      calls.push(`enable ${id} ${enabled}`);
      loaded.find((p) => p.id === id)!.enabled = enabled;
    },
    list: () => loaded.map(info),
    renderTab: async (...args) => {
      lastServices = args[8];
      lastTab = args;
      return { view: { blocks: [] }, state: null, update_request: null };
    },
    importFiles: async (_p, _i, _f, services) => {
      lastServices = services;
      return { name: "x", files: [] };
    },
  };
  return { plugins, loaded, calls, services: () => lastServices, tab: () => lastTab };
}

/** Bundled `id@version` stand-ins with a welcome choice; the record lives in `saved`. */
function fakeBundled(shipped: Record<string, string>, saved: { record: WelcomeRecord | null }): BundledSource {
  return {
    ids: Object.keys(shipped),
    fetch: async (id) => (shipped[id] ? new TextEncoder().encode(`${id}@${shipped[id]}`) : null),
    manifest: async (wasm) => {
      const [id, version] = new TextDecoder().decode(wasm).split("@");
      return {
        api: 1,
        id: id!,
        name: id!,
        version: version!,
        description: "",
        license: null,
        homepage: null,
        permissions: { ...NONE, storage: true },
        contributes: { prepare: false, response: false, tabs: [], importers: [], welcome: { label: `${id} extras`, description: "" } },
      };
    },
    load: () => structuredClone(saved.record ?? { done: false, bundled: {} }),
    save: (r) => void (saved.record = structuredClone(r)),
  };
}

function setup(store: PluginStore = memoryPluginStore(), failCore = false, bundled?: BundledSource) {
  const core = fakeCore();
  const host = {
    deps: {
      loadCore: async () => {
        if (failCore) throw new Error("not built");
        return { Plugins: core.plugins };
      },
    },
    session: () => ({
      id: "c1",
      ws: {
        request: () => ({ url: "x" }),
        responseBytes: () => new Uint8Array([1, 2]),
        pluginScope: (path: string, env?: string) => ({ context: { environment: env ?? null, variables: [] }, request_urls: [path] }),
      },
    }),
    companionCredentials: async () => null,
  } as unknown as WebHost;
  return { api: webPlugins(host, async () => store, bundled ?? fakeBundled({}, { record: null })), host, core, store };
}

describe("web plugins", () => {
  it("installs in order, keeps order and enabled state on update", async () => {
    const { api, core, store } = setup();
    await api.install(file("a"));
    await api.install(file("b"));
    await api.setEnabled("a", false);
    await api.install(file("a", "2.0.0"));
    const stored = (await store.modules()).sort((x, y) => x.order - y.order);
    expect(stored.map((m) => [m.id, m.order, m.enabled])).toEqual([
      ["a", 0, false],
      ["b", 1, true],
    ]);
    expect(core.loaded.find((p) => p.id === "a")).toMatchObject({ version: "2.0.0", enabled: false });
    expect(stored[0]!.granted.storage).toBe(true);
  });

  it("loads stored modules into the core on first use, in installation order", async () => {
    const store = memoryPluginStore();
    await store.putModule({ id: "b", wasm: file("b").data, granted: NONE, enabled: true, order: 1 });
    await store.putModule({ id: "a", wasm: file("a").data, granted: NONE, enabled: false, order: 0 });
    const { api, core } = setup(store);
    expect((await api.list()).map((p) => p.manifest.id)).toEqual(["a", "b"]);
    expect(core.calls).toEqual(["load a", "load b"]);
    expect(core.loaded[0]!.enabled).toBe(false);
  });

  it("removes a plugin with its storage; unknown ids are a plugin error", async () => {
    const { api, store } = setup();
    await api.install(file("a"));
    await store.set("a", "k", 1);
    await store.set("b", "k", 2);
    await api.remove("a");
    expect(await store.modules()).toEqual([]);
    expect(await store.get("a", "k")).toBeUndefined();
    expect(await store.get("b", "k")).toBe(2);
    await expect(api.remove("a")).rejects.toMatchObject({ kind: "plugin" });
    await expect(api.setEnabled("zz", true)).rejects.toMatchObject({ kind: "plugin" });
  });

  it("gives a send the collection, the companion and the plugin storage", async () => {
    const { api, store } = setup();
    const services = (await api.forSend("c1", null)) as PluginServices;
    expect(services.collection).toBe("c1");
    expect(services.companion).toBeNull();
    await services.storage.set("p", "k", { n: 1 });
    expect(await store.get("p", "k")).toEqual({ n: 1 });
    expect(await services.storage.get("p", "k")).toEqual({ n: 1 });
  });

  it("a send without a plugin host runs without plugins", async () => {
    const { api } = setup(memoryPluginStore(), true);
    expect(await api.forSend("c1", null)).toBeUndefined();
    await expect(api.list()).rejects.toMatchObject({ kind: "shell" });
  });

  it("tabs and importers get the session's collection", async () => {
    const { api, core, host } = setup();
    await api.renderTab("a", "t", { request_path: "r.yml", response_id: 3 } as never, null, "DEV");
    expect(core.services()?.collection).toBe("c1");
    expect(core.tab()[5]).toEqual({ url: "x" });
    expect(core.tab()[9]).toEqual({ context: { environment: "DEV", variables: [] }, request_urls: ["r.yml"] });
    // Collection tabs: no request.
    await api.action("a", "t", "go", { request_path: "", response_id: null }, null, [], null);
    expect(core.tab()[5]).toBeNull();
    expect(core.tab()[9]).toEqual({ context: { environment: null, variables: [] }, request_urls: [""] });
    await pluginsOf(host).importReply([], "a", "i");
    expect(core.services()?.collection).toBe("c1");
  });

  it("installs bundled plugins disabled, without the core, and asks once", async () => {
    const store = memoryPluginStore();
    const saved = { record: null as WelcomeRecord | null };
    // `failCore`: nothing here may load the core (no collection is open at first start).
    const page = (version = "1.0.0") => setup(store, true, fakeBundled({ demo: version }, saved)).api;

    let api = page();
    expect(await api.welcome()).toEqual({ done: false, choices: [{ plugin: "demo", choice: { label: "demo extras", description: "" } }] });
    expect((await store.modules()).map((m) => [m.id, m.enabled])).toEqual([["demo", false]]);
    expect(saved.record).toEqual({ done: false, bundled: { demo: "1.0.0" } });

    // Not answered: asked again on the next page, installed once.
    api = page();
    expect((await api.welcome()).done).toBe(false);
    expect(await store.modules()).toHaveLength(1);

    await expect(api.chooseWelcome(["other"])).rejects.toMatchObject({ kind: "plugin" });
    await api.chooseWelcome([]);
    expect((await page().welcome()).done).toBe(true);
    expect((await store.modules())[0]!.enabled).toBe(false);

    // Chosen later (the dialog runs once; the API stays usable), then a newer bundled version.
    await page().chooseWelcome(["demo"]);
    expect((await store.modules())[0]!.enabled).toBe(true);
    await page("1.1.0").welcome();
    const [m] = await store.modules();
    expect([new TextDecoder().decode(m!.wasm), m!.enabled]).toEqual(["demo@1.1.0", true]);

    // Removed by the user: stays removed.
    await store.deleteModule("demo");
    expect(await page("1.2.0").welcome()).toEqual({ done: true, choices: [] });
    expect(await store.modules()).toEqual([]);
  });

  it("reads the manifest of a real module and compares versions", async () => {
    const wasm = readFileSync(new URL("../../../crates/rt-plugin/tests/fixtures/demo-bundled.wasm", import.meta.url));
    const manifest = await embeddedManifest(new Uint8Array(wasm));
    expect(manifest?.contributes.welcome?.label).toBe("Demo extras");
    expect(await embeddedManifest(new TextEncoder().encode("<!doctype html>"))).toBeNull();
    expect(newer("1.10.0", "1.9.0")).toBe(true);
    expect(newer("1.0", "1.0.0")).toBe(false);
    expect(newer("1.0.0", "1.0.0-rc.1")).toBe(true);
    expect(newer("1.0.0-rc.1", "1.0.0")).toBe(false);
  });
});
