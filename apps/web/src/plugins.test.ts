// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web plugin part against a fake core: what is stored, what the core is told, what a
// send gets. The real host (rt-wasm) is covered by rt-plugin's tests and the shell e2e test.

import { describe, expect, it } from "vitest";

import type { Manifest, Permissions, PluginInfo } from "@requesttailor/ui";

import { memoryPluginStore, pluginsOf, webPlugins, type CorePlugins, type PluginServices, type PluginStore } from "./plugins";
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
      contributes: { prepare: false, response: false, tabs: [], importers: [] },
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
      return { view: { blocks: [] }, state: null, update_request: null };
    },
    importFiles: async (_p, _i, _f, services) => {
      lastServices = services;
      return { name: "x", files: [] };
    },
  };
  return { plugins, loaded, calls, services: () => lastServices };
}

function setup(store: PluginStore = memoryPluginStore(), failCore = false) {
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
      ws: { request: () => ({ url: "x" }), responseBytes: () => new Uint8Array([1, 2]) },
    }),
    companionCredentials: async () => null,
  } as unknown as WebHost;
  return { api: webPlugins(host, async () => store), host, core, store };
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
    await api.renderTab("a", "t", { request_path: "r.yml", response_id: 3 } as never, null);
    expect(core.services()?.collection).toBe("c1");
    await pluginsOf(host).importReply([], "a", "i");
    expect(core.services()?.collection).toBe("c1");
  });
});
