// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CatalogueApi` of the web app (wave 18, strand I): the plugin catalogue over rt-wasm
// (`crates/rt-wasm/src/catalogue_js.rs`), the same checks as the desktop app and `rt plugin`.
//
// - The sources the user added live in localStorage (`SOURCES_KEY`), and so does where an
//   installed plugin came from (`INSTALLED_KEY`: source and SHA-256 per plugin id). There is no
//   built-in index, nothing is requested at startup, and no request is made except for a
//   call of this API (Refresh, Install, Check for updates).
// - Downloads use the browser's `fetch`; when the host does not allow it (CORS) and the
//   companion is paired, again through `rt agent`; otherwise the error says how to get the
//   plugin in (the companion, or a file in the plugins dialog). A Git source is read through
//   the host's raw-file address only (a clone needs the CLI or the desktop app).
// - Installing: the core downloads the module again, repeats the checksum and manifest checks
//   and hands back the verified bytes; they go through the ordinary web plugin install
//   (`plugins.ts`: rights from the module's manifest).

import {
  BackendError,
  type CatalogueApi,
  type CatalogueSource,
  type CatalogueUpdate,
  type InstallPlan,
  type InstalledSource,
  type SourceIndex,
} from "@requesttailor/ui";

import { looksBlocked } from "./openapi";
import { pluginsOf } from "./plugins";
import { toBackendError, type CompanionCredentials, type CoreModule } from "./wasm";
import type { WebHost } from "./web-backend";

export const SOURCES_KEY = "requesttailor:catalogue-sources";
export const INSTALLED_KEY = "requesttailor:catalogue-installed";

export const CATALOGUE_CORS_HINT =
  "The server does not let this browser read it (CORS). Pair the companion (run `rt agent` on this computer), or download the plugin file and install it from the plugins dialog.";

/** What rt-wasm exports (`catalogue_js.rs`). */
export interface CoreCatalogue {
  catalogueLocate(source: string): { index_urls: string[]; commit: string | null; git: boolean };
  catalogueFetchIndex(source: string, companion: CompanionCredentials | null): Promise<{ index: SourceIndex; index_url: string }>;
  cataloguePlan(
    source: string,
    id: string,
    installed: InstalledPlugin | null,
    companion: CompanionCredentials | null,
  ): Promise<InstallPlan>;
  catalogueModule(plan: InstallPlan, installed: InstalledPlugin | null, companion: CompanionCredentials | null): Promise<Uint8Array>;
  catalogueUpdates(indexes: SourceIndex[], installed: InstalledSource[]): CatalogueUpdate[];
}

/** The plugin with the entry's id, as the plan needs it. */
interface InstalledPlugin {
  version: string;
  granted: { network: string[]; storage: boolean; variables: boolean };
  sha256: string | null;
}

/** Where the lists are kept (tests replace it). */
export interface CatalogueStorage {
  read(key: string): string | null;
  write(key: string, value: string): void;
}

function browserStorage(): CatalogueStorage {
  return {
    read: (key) => {
      try {
        return localStorage.getItem(key);
      } catch {
        return null;
      }
    },
    write: (key, value) => {
      try {
        localStorage.setItem(key, value);
      } catch (e) {
        console.warn("RequestTailor: the catalogue is not saved in this browser:", e);
      }
    },
  };
}

type Recorded = Record<string, { source: string; sha256: string; version: string }>;

export function webCatalogue(
  host: WebHost,
  companionOn: () => boolean = () => false,
  storage: CatalogueStorage = browserStorage(),
): CatalogueApi {
  const core = async () => (await host.deps.loadCore()) as CoreModule & CoreCatalogue;

  const readSources = (): CatalogueSource[] => {
    try {
      const v = JSON.parse(storage.read(SOURCES_KEY) ?? "[]") as unknown;
      return Array.isArray(v) ? (v as CatalogueSource[]) : [];
    } catch {
      return [];
    }
  };
  const writeSources = (s: CatalogueSource[]) => storage.write(SOURCES_KEY, JSON.stringify(s));
  const readRecorded = (): Recorded => {
    try {
      const v = JSON.parse(storage.read(INSTALLED_KEY) ?? "{}") as unknown;
      return v && typeof v === "object" ? (v as Recorded) : {};
    } catch {
      return {};
    }
  };
  const writeRecorded = (r: Recorded) => storage.write(INSTALLED_KEY, JSON.stringify(r));

  const guard = async <T>(f: () => Promise<T>): Promise<T> => {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  };

  /** Runs `f` with the browser's fetch; when that is blocked, through the companion. */
  async function network<T>(f: (companion: CompanionCredentials | null) => Promise<T>): Promise<T> {
    if (companionOn()) return f(await host.companionCredentials());
    try {
      return await f(null);
    } catch (e) {
      const err = toBackendError(e);
      if (!looksBlocked(err.message)) throw err;
      const companion = await host.companionCredentials();
      if (companion) return f(companion);
      throw new BackendError(err.kind, `${err.message}. ${CATALOGUE_CORS_HINT}`);
    }
  }

  /** The installed plugin with this id as the plan needs it, or null. */
  async function installedPlugin(id: string): Promise<InstalledPlugin | null> {
    const plugin = (await pluginsOf(host).list()).find((p) => p.manifest.id === id);
    if (!plugin) return null;
    const rec = readRecorded()[id];
    return {
      version: plugin.manifest.version,
      granted: { network: [], storage: false, variables: false, ...(plugin.granted ? plugin.manifest.permissions : {}) },
      sha256: rec && rec.version === plugin.manifest.version ? rec.sha256 : null,
    };
  }

  async function installedSources(): Promise<InstalledSource[]> {
    const have = new Map((await pluginsOf(host).list()).map((p) => [p.manifest.id, p.manifest.version]));
    const out: InstalledSource[] = [];
    for (const [plugin, r] of Object.entries(readRecorded())) {
      if (have.get(plugin) === r.version) out.push({ plugin, source: r.source, version: r.version, sha256: r.sha256 });
    }
    return out;
  }

  return {
    sources: async () => readSources(),
    addSource: (url) =>
      guard(async () => {
        const c = await core();
        c.catalogueLocate(url); // checks what was typed; no request
        const typed = url.trim();
        const all = readSources();
        const existing = all.find((s) => s.url === typed);
        if (existing) return existing;
        const source: CatalogueSource = { url: typed, commit: null, name: null };
        writeSources([...all, source]);
        return source;
      }),
    removeSource: async (url) => {
      const all = readSources();
      if (!all.some((s) => s.url === url)) throw new BackendError("plugin", `${url} is not one of your sources`);
      writeSources(all.filter((s) => s.url !== url));
    },
    refresh: (source) =>
      guard(async () => {
        const c = await core();
        const all = readSources();
        const wanted = source === undefined ? all : all.filter((s) => s.url === source);
        if (source !== undefined && wanted.length === 0) throw new BackendError("plugin", `${source} is not one of your sources`);
        const out: SourceIndex[] = [];
        for (const s of wanted) {
          const fetched = await network((co) => c.catalogueFetchIndex(s.url, co));
          out.push(fetched.index);
          writeSources(readSources().map((x) => (x.url === s.url ? fetched.index.source : x)));
        }
        return out;
      }),
    plan: (source, id) =>
      guard(async () => {
        const c = await core();
        const installed = await installedPlugin(id);
        return network((co) => c.cataloguePlan(source, id, installed, co));
      }),
    install: (plan) =>
      guard(async () => {
        const c = await core();
        const id = plan.entry.id;
        const installed = await installedPlugin(id);
        const bytes = await network((co) => c.catalogueModule(plan, installed, co));
        const info = await pluginsOf(host).install({ name: `${id}.wasm`, data: bytes });
        const record = { source: plan.source, sha256: plan.sha256, version: info.manifest.version };
        writeRecorded({ ...readRecorded(), [info.manifest.id]: record });
        return { plugin: info.manifest.id, source: record.source, version: record.version, sha256: record.sha256 };
      }),
    installed: () => guard(installedSources),
    checkUpdates: () =>
      guard(async () => {
        const c = await core();
        const installed = await installedSources();
        const wanted = [...new Set(installed.map((i) => i.source))];
        const indexes: SourceIndex[] = [];
        for (const url of wanted) indexes.push((await network((co) => c.catalogueFetchIndex(url, co))).index);
        return c.catalogueUpdates(indexes, installed);
      }),
  };
}
