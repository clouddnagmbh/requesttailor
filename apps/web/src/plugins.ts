// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `PluginApi` of the web app (wave 5, strand A): plugin modules in browser storage, run by
// rt-plugin's host inside rt-wasm (the same interpreter as natively).
//
// - Modules, their grants and enabled state live in IndexedDB `requesttailor-plugins`
//   (store `modules`), the plugins' `storage` in store `storage` (key `<id>\0<key>`). Without
//   IndexedDB (private windows, blocked site data) plugins live for this page only.
// - The core keeps the compiled plugins for the page (`Plugins` in rt-wasm); they are loaded
//   from IndexedDB on first use.
// - Plugin `http` calls go through the companion when it is paired (no CORS), else `fetch`.

import { BackendError, type ImportSource, type Manifest, type Permissions, type PluginApi, type PluginInfo } from "@requesttailor/ui";

import { toBackendError, type CoreModule } from "./wasm";
import type { WebHost } from "./web-backend";

/** Bodies handed to plugins are cut at this size (rt_plugin::host::PLUGIN_BODY_LIMIT). */
export const PLUGIN_BODY_LIMIT = 8 * 1024 * 1024;

export const PLUGINS_DB = "requesttailor-plugins";

/** `Plugins` of rt-wasm (crates/rt-wasm/src/plugins_js.rs). */
export interface CorePlugins {
  inspect(wasm: Uint8Array): Manifest;
  load(wasm: Uint8Array, granted: Permissions, enabled: boolean): PluginInfo;
  unload(id: string): void;
  setEnabled(id: string, enabled: boolean): void;
  list(): PluginInfo[];
  renderTab(
    plugin: string,
    tab: string,
    action: string | null,
    input: [string, string][],
    target: unknown,
    spec: unknown,
    body: Uint8Array | null,
    state: unknown,
    services: PluginServices,
  ): Promise<unknown>;
  importFiles(plugin: string, importer: string, files: ImportSource[], services: PluginServices): Promise<unknown>;
}

/** The `plugins` field of the services object (`plugins_js.rs`). */
export interface PluginServices {
  collection: string;
  companion: { url: string; token: string } | null;
  storage: {
    get(plugin: string, key: string): Promise<unknown>;
    set(plugin: string, key: string, value: unknown): Promise<void>;
  };
}

/** One installed module as stored. */
export interface StoredModule {
  id: string;
  wasm: Uint8Array;
  granted: Permissions;
  enabled: boolean;
  /** Installation order. */
  order: number;
}

/** Where modules and plugin storage are kept. */
export interface PluginStore {
  modules(): Promise<StoredModule[]>;
  putModule(m: StoredModule): Promise<void>;
  deleteModule(id: string): Promise<void>;
  get(plugin: string, key: string): Promise<unknown>;
  set(plugin: string, key: string, value: unknown): Promise<void>;
}

/** `PluginApi` plus the plugin part of a send (the `plugins` field `services_js` reads). */
export interface WebPluginApi extends PluginApi {
  forSend(collectionId: string, environment: string | null): Promise<unknown>;
  /** Runs a plugin importer; resolves with its `ImportReply` for `Importer.addPluginImport`. */
  importReply(files: ImportSource[], plugin: string, importer: string): Promise<unknown>;
}

function req<T>(r: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    r.onsuccess = () => resolve(r.result);
    r.onerror = () => reject(r.error);
  });
}

function done(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
    tx.onabort = () => reject(tx.error);
  });
}

/** Keeps everything in memory (no IndexedDB, tests). */
export function memoryPluginStore(): PluginStore {
  const modules = new Map<string, StoredModule>();
  const values = new Map<string, unknown>();
  return {
    modules: async () => [...modules.values()],
    putModule: async (m) => void modules.set(m.id, m),
    deleteModule: async (id) => {
      modules.delete(id);
      for (const k of [...values.keys()]) if (k.startsWith(`${id}\0`)) values.delete(k);
    },
    get: async (plugin, key) => values.get(`${plugin}\0${key}`),
    set: async (plugin, key, value) => {
      if (value === undefined) values.delete(`${plugin}\0${key}`);
      else values.set(`${plugin}\0${key}`, value);
    },
  };
}

async function openIndexedDb(): Promise<IDBDatabase> {
  const open = indexedDB.open(PLUGINS_DB, 1);
  open.onupgradeneeded = () => {
    open.result.createObjectStore("modules", { keyPath: "id" });
    open.result.createObjectStore("storage");
  };
  return req(open);
}

/** IndexedDB, or memory if the browser refuses it. */
export async function openPluginStore(): Promise<PluginStore> {
  let db: IDBDatabase;
  try {
    db = await openIndexedDb();
  } catch (e) {
    console.warn("RequestTailor: plugins are not saved in this browser:", e);
    return memoryPluginStore();
  }
  const store = (name: string, mode: IDBTransactionMode) => {
    const tx = db.transaction(name, mode);
    return { s: tx.objectStore(name), tx };
  };
  return {
    modules: async () => (await req(store("modules", "readonly").s.getAll())) as StoredModule[],
    putModule: async (m) => {
      const { s, tx } = store("modules", "readwrite");
      s.put(m);
      await done(tx);
    },
    deleteModule: async (id) => {
      const { s, tx } = store("modules", "readwrite");
      s.delete(id);
      await done(tx);
      const st = store("storage", "readwrite");
      st.s.delete(IDBKeyRange.bound(`${id}\0`, `${id}\0￿`));
      await done(st.tx);
    },
    get: async (plugin, key) => {
      try {
        return await req(store("storage", "readonly").s.get(`${plugin}\0${key}`));
      } catch {
        return undefined;
      }
    },
    set: async (plugin, key, value) => {
      const { s, tx } = store("storage", "readwrite");
      if (value === undefined) s.delete(`${plugin}\0${key}`);
      else s.put(value, `${plugin}\0${key}`);
      await done(tx);
    },
  };
}

function corePlugins(core: CoreModule): CorePlugins {
  const plugins = (core as unknown as { Plugins?: CorePlugins }).Plugins;
  if (!plugins) throw new BackendError("shell", "This RequestTailor core has no plugin host; rebuild it.");
  return plugins;
}

function notInstalled(id: string): BackendError {
  return new BackendError("plugin", `plugin ${id} is not installed`);
}

/** Each backend's plugin part, so `./import.ts` reaches the same instance. */
const byHost = new WeakMap<WebHost, WebPluginApi>();

export function webPlugins(host: WebHost, openStore: () => Promise<PluginStore> = openPluginStore): WebPluginApi {
  let store: Promise<PluginStore> | null = null;
  let ready: Promise<CorePlugins> | null = null;
  const modules = () => (store ??= openStore());

  /** The core's plugin host with the stored modules loaded (once per page). */
  const core = () =>
    (ready ??= (async () => {
      const plugins = corePlugins(await host.deps.loadCore());
      const stored = (await (await modules()).modules()).sort((a, b) => a.order - b.order);
      for (const m of stored) {
        try {
          plugins.load(m.wasm, m.granted, m.enabled);
        } catch (e) {
          console.warn(`RequestTailor: plugin ${m.id} not loaded:`, e);
        }
      }
      return plugins;
    })().catch((e: unknown) => {
      ready = null;
      throw e;
    }));

  async function find(id: string): Promise<StoredModule> {
    const m = (await (await modules()).modules()).find((x) => x.id === id);
    if (!m) throw notInstalled(id);
    return m;
  }

  const services = async (collection: string): Promise<PluginServices> => {
    const s = await modules();
    return {
      collection,
      companion: await host.companionCredentials(),
      storage: { get: (p, k) => s.get(p, k), set: (p, k, v) => s.set(p, k, v) },
    };
  };

  const guard = async <T>(f: () => Promise<T>): Promise<T> => {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  };

  const tab = (plugin: string, tab: string, action: string | null, target: import("@requesttailor/ui").TabTarget, state: unknown, input: [string, string][]) =>
    guard(async () => {
      const plugins = await core();
      const s = host.session();
      const spec = s.ws.request(target.request_path);
      const body = target.response_id == null ? null : s.ws.responseBytes(target.response_id, 0, PLUGIN_BODY_LIMIT + 1);
      return (await plugins.renderTab(plugin, tab, action, input, target, spec, body, state ?? null, await services(s.id))) as import("@requesttailor/ui").ViewReply;
    });

  const api: WebPluginApi = {
    list: () => guard(async () => (await core()).list()),
    inspect: (file) => guard(async () => corePlugins(await host.deps.loadCore()).inspect(file.data)),
    install: (file) =>
      guard(async () => {
        const plugins = await core();
        const manifest = plugins.inspect(file.data);
        const s = await modules();
        const all = await s.modules();
        const existing = all.find((m) => m.id === manifest.id);
        const enabled = existing?.enabled ?? true;
        const info = plugins.load(file.data, manifest.permissions, enabled);
        await s.putModule({
          id: manifest.id,
          wasm: new Uint8Array(file.data),
          granted: manifest.permissions,
          enabled,
          order: existing?.order ?? all.reduce((n, m) => Math.max(n, m.order + 1), 0),
        });
        return info;
      }),
    remove: (id) =>
      guard(async () => {
        const plugins = await core();
        await find(id);
        plugins.unload(id);
        await (await modules()).deleteModule(id);
      }),
    setEnabled: (id, enabled) =>
      guard(async () => {
        const plugins = await core();
        const m = await find(id);
        plugins.setEnabled(id, enabled);
        await (await modules()).putModule({ ...m, enabled });
      }),
    // `environment` (wave 6, strand F): not passed to the plugin's context yet.
    renderTab: (plugin, t, target, state) => tab(plugin, t, null, target, state, []),
    action: (plugin, t, action, target, state, input) => tab(plugin, t, action, target, state, input),
    // Wave 6, strand F: bundled plugins and the first-start question. Interface commit:
    // answered already, so no dialog shows.
    welcome: async () => ({ done: true, choices: [] }),
    chooseWelcome: async () => {},
    forSend: async (collectionId) => {
      try {
        await core();
        return await services(collectionId);
      } catch (e) {
        // Plugins must never stop a send of their own accord; without a host none run.
        console.warn("RequestTailor: plugins unavailable for this send:", e);
        return undefined;
      }
    },
    importReply: (files, plugin, importer) =>
      guard(async () => {
        const plugins = await core();
        let collection = "";
        try {
          collection = host.session().id;
        } catch {
          // Importing needs no open collection.
        }
        return plugins.importFiles(plugin, importer, files, await services(collection));
      }),
  };
  byHost.set(host, api);
  return api;
}

/** The plugin part of the backend `host` belongs to (for `./import.ts`). */
export function pluginsOf(host: WebHost): WebPluginApi {
  const api = byHost.get(host);
  if (!api) throw new BackendError("shell", "plugins are not set up");
  return api;
}
