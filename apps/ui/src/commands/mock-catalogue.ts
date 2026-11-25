// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CatalogueApi` of the mock backend (wave 18, strand I): a small mirror of `rt_app::catalogue`
// for UI tests (`?backend=mock`). It has no built-in index (nothing is listed until the user
// adds a source and refreshes), reads nothing from the network, and answers every source with the
// same canned index, except an address with "unreachable" in it (the read fails). The core is the
// reference; this only has to behave the same way for the screens.
//
// Canned entries: `json-tools` 1.2.0 (storage), `acme-lint` 2.0.0 (network), `flaky` 1.0.0 (its
// download fails the checksum check) and `sneaky` 1.0.0 (the index claims no rights, the module
// asks for network access).

import { BackendError, type CatalogueApi, type PluginApi } from "./backend";
import { mockManifest, mockPluginFile } from "./mock-plugins";
import type {
  CatalogueEntry,
  CatalogueSource,
  CatalogueUpdate,
  InstallPlan,
  InstalledSource,
  Permissions,
  SourceIndex,
} from "./types";

const none: Permissions = { network: [], storage: false, variables: false };

function sum(id: string, version: string): string {
  let h = 0x811c9dc5;
  for (const c of `${id}@${version}`) h = Math.imul(h ^ c.charCodeAt(0), 0x01000193) >>> 0;
  return h.toString(16).padStart(8, "0").repeat(8);
}

interface Canned {
  entry: CatalogueEntry;
  /** What the module itself asks for (the index may claim otherwise). */
  module: Permissions;
}

function canned(id: string, name: string, version: string, description: string, claimed: Permissions, module = claimed): Canned {
  return {
    entry: {
      id,
      name,
      version,
      api: 1,
      description,
      license: "Apache-2.0",
      homepage: null,
      wasm: { url: `${id}-${version}.wasm`, sha256: sum(id, version), size: 4096 },
      permissions: claimed,
    },
    module,
  };
}

export const MOCK_CATALOGUE: Canned[] = [
  canned("json-tools", "JSON tools", "1.2.0", "Pretty-print and query JSON bodies", { ...none, storage: true }),
  canned("acme-lint", "Acme lint", "2.0.0", "Checks requests against the Acme API rules", { ...none, network: ["api.acme.test"] }),
  canned("flaky", "Flaky", "1.0.0", "Its download does not match the index", none),
  canned("sneaky", "Sneaky", "1.0.0", "Claims no rights, asks for network access", none, { ...none, network: ["*"] }),
];

const newer = (a: string, b: string) => {
  const x = a.split(".").map(Number);
  const y = b.split(".").map(Number);
  for (let i = 0; i < Math.max(x.length, y.length); i++) if ((x[i] ?? 0) !== (y[i] ?? 0)) return (x[i] ?? 0) > (y[i] ?? 0);
  return false;
};

const covers = (granted: Permissions, wanted: Permissions) =>
  (granted.storage || !wanted.storage) &&
  (granted.variables || !wanted.variables) &&
  wanted.network.every((n) => granted.network.includes(n));

export function mockCatalogue(plugins?: () => PluginApi): CatalogueApi {
  let sources: CatalogueSource[] = [];
  const installed = new Map<string, InstalledSource>();

  const known = (url: string) => {
    if (!sources.some((s) => s.url === url)) throw new BackendError("plugin", `${url} is not one of your sources`);
  };
  const find = (id: string) => MOCK_CATALOGUE.find((c) => c.entry.id === id);

  async function current(id: string) {
    return (await plugins?.().list())?.find((p) => p.manifest.id === id);
  }

  return {
    sources: async () => sources.map((s) => ({ ...s })),
    addSource: async (url) => {
      const typed = url.trim();
      if (!/^https:\/\/\S+$/.test(typed) && !/^http:\/\/(127\.0\.0\.1|localhost)[:/]/.test(typed)) {
        throw new BackendError("plugin", "a source is an https:// address without a password (http only for localhost)");
      }
      const existing = sources.find((s) => s.url === typed);
      if (existing) return { ...existing };
      const source: CatalogueSource = { url: typed, commit: null, name: null };
      sources = [...sources, source];
      return { ...source };
    },
    removeSource: async (url) => {
      known(url);
      sources = sources.filter((s) => s.url !== url);
    },
    refresh: async (url) => {
      const wanted = url === undefined ? sources : sources.filter((s) => s.url === url);
      if (url !== undefined) known(url);
      const out: SourceIndex[] = [];
      for (const s of wanted) {
        if (s.url.includes("unreachable")) throw new BackendError("plugin", `cannot read the index of ${s.url}: HTTP 404`);
        const git = !s.url.endsWith(".json");
        const fresh: CatalogueSource = { url: s.url, commit: git ? "3fa1c9e" : null, name: "Mock index" };
        sources = sources.map((x) => (x.url === s.url ? fresh : x));
        out.push({ source: fresh, index: { version: 1, name: "Mock index", plugins: MOCK_CATALOGUE.map((c) => structuredClone(c.entry)) } });
      }
      return out;
    },
    plan: async (source, id) => {
      known(source);
      const c = find(id);
      if (!c) throw new BackendError("plugin", `${source} lists no plugin "${id}"`);
      if (id === "flaky") {
        throw new BackendError("plugin", `${id}: the checksum does not match (the index says ${c.entry.wasm.sha256}, the download is ${"0".repeat(64)}). Nothing was installed.`);
      }
      const have = await current(id);
      const differ = JSON.stringify(c.module) !== JSON.stringify(c.entry.permissions ?? none);
      const warnings = differ ? ["The index names other rights than the plugin itself asks for; the plugin's own are shown."] : [];
      const plan: InstallPlan = {
        source,
        entry: structuredClone(c.entry),
        wasm_url: `${source.replace(/[^/]*$/, "")}${c.entry.wasm.url}`,
        sha256: c.entry.wasm.sha256,
        permissions: c.module,
        permissions_differ: differ,
        update: have !== undefined,
        more_permissions: have !== undefined && !covers(have.manifest.permissions, c.module),
        warnings,
      };
      return plan;
    },
    install: async (plan) => {
      const c = find(plan.entry.id);
      if (!c) throw new BackendError("plugin", `no plugin ${plan.entry.id}`);
      if (!plugins) throw new BackendError("unsupported", "The mock has no plugin host.");
      await plugins().install(
        mockPluginFile(mockManifest({ id: c.entry.id, name: c.entry.name, version: c.entry.version, description: c.entry.description ?? "", permissions: c.module })),
      );
      const record: InstalledSource = { plugin: c.entry.id, source: plan.source, version: c.entry.version, sha256: plan.sha256 };
      installed.set(record.plugin, record);
      return { ...record };
    },
    installed: async () => [...installed.values()].map((i) => ({ ...i })),
    checkUpdates: async () => {
      const out: CatalogueUpdate[] = [];
      for (const i of installed.values()) {
        const c = find(i.plugin);
        const have = await current(i.plugin);
        if (c && have && newer(c.entry.version, have.manifest.version)) {
          out.push({ plugin: i.plugin, source: i.source, installed: have.manifest.version, available: c.entry.version });
        }
      }
      return out;
    },
  };
}
