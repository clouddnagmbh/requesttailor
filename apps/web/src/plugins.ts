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
// - Tabs (wave 6) get the selected environment: the core builds their context and the URLs
//   `{request}` covers (`Workspace.pluginScope`); collection tabs have no request.
// - Bundled plugins (wave 6): the build puts them next to the app (`plugins/<id>.wasm`,
//   `npm run bundle-plugins -w apps/web`). `welcome()` installs them into IndexedDB,
//   disabled, or updates an older installed version, without loading the core (only
//   IndexedDB and a fetch; a missing file, or the dev server's HTML fallback, means none).
//   The answer and the installed bundled versions are kept in localStorage
//   (`WELCOME_KEY`), so a bundled plugin the user removed stays removed.

import {
  BackendError,
  type ImportSource,
  type Manifest,
  type Permissions,
  type PluginApi,
  type PluginInfo,
  type TabTarget,
  type ViewReply,
  type WelcomeState,
} from "@requesttailor/ui";

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
    scope?: unknown,
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

/** The plugins the web app ships (`plugins/<id>.wasm` next to the app). */
export const BUNDLED = ["odata"];
/** localStorage key of the first-start answer and the installed bundled versions. */
export const WELCOME_KEY = "requesttailor:welcome";

/** What `welcome.json` is on the desktop (`rt_app::plugins::WelcomeRecord`). */
export interface WelcomeRecord {
  done: boolean;
  /** Bundled plugin id → version installed from the app. */
  bundled: Record<string, string>;
}

/** Where bundled plugins and the welcome record come from (tests replace it). */
export interface BundledSource {
  ids: string[];
  /** The module of bundled plugin `id`, or `null` if this build has none. */
  fetch(id: string): Promise<Uint8Array | null>;
  /** The manifest embedded in a module, or `null` if it is none. */
  manifest(wasm: Uint8Array): Promise<Manifest | null>;
  load(): WelcomeRecord;
  save(record: WelcomeRecord): void;
}

/** The manifest in the `rt-plugin` custom section, without the core. */
export async function embeddedManifest(wasm: Uint8Array): Promise<Manifest | null> {
  try {
    const module = await WebAssembly.compile(wasm as BufferSource);
    const [section] = WebAssembly.Module.customSections(module, "rt-plugin");
    return section ? (JSON.parse(new TextDecoder().decode(section)) as Manifest) : null;
  } catch {
    return null;
  }
}

/** Whether version `a` is newer than `b` (as `rt_app::plugins::newer`). */
export function newer(a: string, b: string): boolean {
  const parts = (v: string): [number[], boolean] => {
    const m = /^([^-+]*)([-+])?(.*)$/.exec(v)!;
    const pre = m[2] === "-" && m[3] !== "";
    return [m[1]!.split(".").map((p) => Number.parseInt(p.trim(), 10) || 0), pre];
  };
  const [x, xp] = parts(a);
  const [y, yp] = parts(b);
  for (let i = 0; i < Math.max(x.length, y.length); i++) {
    const d = (x[i] ?? 0) - (y[i] ?? 0);
    if (d !== 0) return d > 0;
  }
  return yp && !xp;
}

function browserBundled(): BundledSource {
  return {
    ids: BUNDLED,
    fetch: async (id) => {
      try {
        const r = await fetch(new URL(`plugins/${id}.wasm`, document.baseURI));
        if (!r.ok) return null;
        const bytes = new Uint8Array(await r.arrayBuffer());
        // "\0asm": not the dev server's HTML fallback.
        return bytes[0] === 0 && bytes[1] === 0x61 && bytes[2] === 0x73 && bytes[3] === 0x6d ? bytes : null;
      } catch {
        return null;
      }
    },
    manifest: embeddedManifest,
    load: () => {
      try {
        const r = JSON.parse(localStorage.getItem(WELCOME_KEY) ?? "null") as Partial<WelcomeRecord> | null;
        return { done: r?.done === true, bundled: r?.bundled && typeof r.bundled === "object" ? r.bundled : {} };
      } catch {
        return { done: false, bundled: {} };
      }
    },
    save: (record) => {
      try {
        localStorage.setItem(WELCOME_KEY, JSON.stringify(record));
      } catch (e) {
        console.warn("RequestTailor: the first-start answer is not saved in this browser:", e);
      }
    },
  };
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

export function webPlugins(
  host: WebHost,
  openStore: () => Promise<PluginStore> = openPluginStore,
  bundled: BundledSource = browserBundled(),
): WebPluginApi {
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

  const tab = (
    plugin: string,
    tab: string,
    action: string | null,
    target: TabTarget,
    state: unknown,
    input: [string, string][],
    environment: string | null | undefined,
  ) =>
    guard(async () => {
      const plugins = await core();
      const s = host.session();
      // Collection tabs (`""`) have no request.
      const spec = target.request_path === "" ? null : s.ws.request(target.request_path);
      const body = target.response_id == null ? null : s.ws.responseBytes(target.response_id, 0, PLUGIN_BODY_LIMIT + 1);
      const scope = s.ws.pluginScope(target.request_path, environment ?? undefined);
      return (await plugins.renderTab(plugin, tab, action, input, target, spec, body, state ?? null, await services(s.id), scope)) as ViewReply;
    });

  // ---- bundled plugins and the first-start question (wave 6) ----
  let record: WelcomeRecord | null = null;
  let installed: Promise<void> | null = null;
  const welcomeRecord = () => (record ??= bundled.load());

  /** Installs or updates the bundled plugins, once per page. */
  const ensureBundled = () =>
    (installed ??= (async () => {
      const s = await modules();
      const next: WelcomeRecord = structuredClone(welcomeRecord());
      for (const id of bundled.ids) {
        const wasm = await bundled.fetch(id);
        const shipped = wasm && (await bundled.manifest(wasm));
        if (!wasm || !shipped || shipped.id !== id) continue;
        const all = await s.modules();
        const existing = all.find((m) => m.id === id);
        const version = existing ? (await bundled.manifest(existing.wasm))?.version : undefined;
        const install = !existing && !(id in next.bundled);
        const update = !!existing && version !== undefined && newer(shipped.version, version);
        if (install || update) {
          const enabled = existing?.enabled ?? false;
          await s.putModule({
            id,
            wasm: new Uint8Array(wasm),
            granted: shipped.permissions,
            enabled,
            order: existing?.order ?? all.reduce((n, m) => Math.max(n, m.order + 1), 0),
          });
          // Already loaded into the core: load the new module too.
          if (ready) (await ready).load(wasm, shipped.permissions, enabled);
          next.bundled[id] = shipped.version;
        } else if (existing && version !== undefined) {
          next.bundled[id] = version;
        }
      }
      if (JSON.stringify(next) !== JSON.stringify(welcomeRecord())) {
        record = next;
        bundled.save(next);
      }
    })().catch((e: unknown) => {
      installed = null;
      throw e;
    }));

  const welcome = async (): Promise<WelcomeState> => {
    await ensureBundled();
    const r = welcomeRecord();
    const s = await modules();
    const choices: WelcomeState["choices"] = [];
    for (const m of (await s.modules()).sort((a, b) => a.order - b.order)) {
      if (!(m.id in r.bundled)) continue;
      const choice = (await bundled.manifest(m.wasm))?.contributes.welcome;
      if (choice) choices.push({ plugin: m.id, choice });
    }
    return { done: r.done, choices };
  };

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
    renderTab: (plugin, t, target, state, environment) => tab(plugin, t, null, target, state, [], environment),
    action: (plugin, t, action, target, state, input, environment) => tab(plugin, t, action, target, state, input, environment),
    welcome: () => guard(welcome),
    chooseWelcome: (ids) =>
      guard(async () => {
        const offered = (await welcome()).choices;
        const unknown = ids.find((id) => !offered.some((o) => o.plugin === id));
        if (unknown) throw new BackendError("plugin", `${unknown} is not a bundled plugin`);
        const s = await modules();
        for (const id of ids) {
          const m = await find(id);
          await s.putModule({ ...m, enabled: true });
          if (ready) (await ready).setEnabled(id, true);
        }
        record = { ...welcomeRecord(), done: true };
        bundled.save(record);
      }),
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
