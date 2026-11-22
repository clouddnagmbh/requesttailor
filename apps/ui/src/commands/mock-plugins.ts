// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Plugins in the mock backend (wave 5, strand A owns this file; strand B adds the example
// plugin's tab replies for its UI tests).
//
// The mock runs no WebAssembly. A "plugin file" for the mock is the manifest as JSON text
// (`mockPluginFile`); anything else is rejected like an invalid plugin. Installed plugins:
// - add notes to every send (`prepare` and `response`, see `notes`);
// - render every tab with one block of each kind; actions: `hello` (a notice), `form`
//   submit (echoes the input), `add-header` (request tabs: an `update_request` that adds
//   `X-Mock-Plugin`), tree items (`item:<label>`);
// - import with every importer: one request per file.

import { BackendError, type ImportSource, type PluginApi } from "./backend";
import type { Manifest, Note, PluginInfo, RequestSpec, TabTarget, View, ViewReply } from "./types";

/** A `.wasm` stand-in for the mock: the manifest as JSON. */
export function mockPluginFile(manifest: Manifest, name = `${manifest.id}.wasm`): ImportSource {
  return { name, data: new TextEncoder().encode(JSON.stringify(manifest)) };
}

/** A complete manifest from the parts a test cares about. */
export function mockManifest(part: Partial<Manifest> & { id: string }): Manifest {
  return {
    name: part.id,
    version: "1.0.0",
    api: 1,
    description: "",
    license: null,
    homepage: null,
    ...part,
    permissions: { network: [], storage: false, variables: false, ...part.permissions },
    contributes: {
      prepare: false,
      response: false,
      tabs: [],
      importers: [],
      ...part.contributes,
      welcome: part.contributes?.welcome ?? null,
    },
  };
}

function parse(file: ImportSource): Manifest {
  let m: Partial<Manifest>;
  try {
    m = JSON.parse(new TextDecoder().decode(file.data)) as Partial<Manifest>;
  } catch {
    throw new BackendError("plugin", `${file.name} is not a RequestTailor plugin: no rt-plugin manifest`);
  }
  if (typeof m?.id !== "string" || !/^[a-z0-9][a-z0-9-]{0,63}$/.test(m.id)) {
    throw new BackendError("plugin", `${file.name} is not a RequestTailor plugin: invalid id`);
  }
  return mockManifest(m as Manifest);
}

export interface MockPlugins extends PluginApi {
  /** Notes the enabled plugins add to a send (`prepare` first, then `response`). */
  notes(): Note[];
  /** `spec` → `RequestSpec` of the request for tabs (set by the mock backend). */
  requestOf?: (path: string) => Promise<RequestSpec>;
}

function view(manifest: Manifest, tab: string, target: TabTarget, calls: number): View {
  return {
    blocks: [
      { type: "heading", text: `${manifest.name}: ${tab}` },
      { type: "text", text: `Request ${target.request_path}${target.response_id === null ? "" : `, response ${target.response_id}`}. Render ${calls}.` },
      { type: "key-value", rows: [["Plugin", manifest.id], ["Version", manifest.version]] },
      { type: "table", columns: ["Name", "Value"], rows: [["a", "1"], ["b", "2"]] },
      { type: "code", language: "json", text: '{"mock": true}' },
      {
        type: "tree",
        nodes: [{ label: "root", detail: "folder", action: null, children: [{ label: "leaf", detail: null, action: "item:leaf", children: [] }] }],
      },
      {
        type: "form",
        id: "form",
        submit: "Run",
        fields: [
          { name: "query", label: "Query", kind: "text", value: "", options: [] },
          { name: "mode", label: "Mode", kind: "select", value: "fast", options: ["fast", "slow"] },
          { name: "verbose", label: "Verbose", kind: "checkbox", value: "false", options: [] },
        ],
      },
      { type: "actions", actions: [{ id: "hello", label: "Say hello" }, { id: "add-header", label: "Add header" }] },
    ],
  };
}

export function mockPlugins(): MockPlugins {
  const installed: PluginInfo[] = [];
  const find = (id: string) => {
    const p = installed.find((x) => x.manifest.id === id);
    if (!p) throw new BackendError("plugin", `plugin ${id} is not installed`);
    return p;
  };
  const tabOf = (id: string, tab: string) => {
    const p = find(id);
    if (!p.enabled) throw new BackendError("plugin", `plugin ${id} is disabled`);
    if (!p.manifest.contributes.tabs.some((t) => t.id === tab)) {
      throw new BackendError("plugin", `plugin ${id} has no tab ${tab}`);
    }
    return p.manifest;
  };
  const count = (state: unknown) => (typeof state === "number" ? state : 0) + 1;

  const api: MockPlugins = {
    list: async () => installed.map((p) => structuredClone(p)),
    inspect: async (file) => parse(file),
    install: async (file) => {
      const manifest = parse(file);
      const at = installed.findIndex((p) => p.manifest.id === manifest.id);
      const info: PluginInfo = { manifest, granted: true, enabled: at >= 0 ? installed[at]!.enabled : true };
      if (at >= 0) installed[at] = info;
      else installed.push(info);
      return structuredClone(info);
    },
    remove: async (id) => {
      find(id);
      installed.splice(
        installed.findIndex((p) => p.manifest.id === id),
        1,
      );
    },
    setEnabled: async (id, enabled) => {
      find(id).enabled = enabled;
    },
    // Wave 6, strand F: bundled plugins and the first-start question. Interface commit:
    // answered already, so no dialog shows.
    welcome: async () => ({ done: true, choices: [] }),
    chooseWelcome: async () => {},
    renderTab: async (plugin, tab, target, state): Promise<ViewReply> => {
      const m = tabOf(plugin, tab);
      const n = count(state);
      return { view: view(m, tab, target, n), state: n, update_request: null, create_requests: [], add_environments: [] };
    },
    action: async (plugin, tab, action, target, state, input): Promise<ViewReply> => {
      const m = tabOf(plugin, tab);
      const n = count(state);
      const reply: ViewReply = {
        view: view(m, tab, target, n),
        state: n,
        update_request: null,
        create_requests: [],
        add_environments: [],
      };
      const say = (text: string) => reply.view.blocks.unshift({ type: "notice", level: "info", text });
      if (action === "hello") say(`Hello from ${m.name}`);
      else if (action === "form") say(`Form: ${input.map(([k, v]) => `${k}=${v}`).join(", ")}`);
      else if (action.startsWith("item:")) say(`Clicked ${action.slice(5)}`);
      else if (action === "add-header") {
        if (!api.requestOf) throw new BackendError("plugin", "no request");
        const spec = await api.requestOf(target.request_path);
        spec.headers.push({ name: "X-Mock-Plugin", value: m.version, enabled: true });
        reply.update_request = spec;
        say("Header added");
      } else throw new BackendError("plugin", `plugin ${plugin}: unknown action ${action}`);
      return reply;
    },
    notes: () => {
      const on = installed.filter((p) => p.enabled);
      return [
        ...on
          .filter((p) => p.manifest.contributes.prepare)
          .map((p): Note => ({ source: `plugin:${p.manifest.id}`, level: "info", text: `${p.manifest.name} prepared the request` })),
        ...on
          .filter((p) => p.manifest.contributes.response)
          .map((p): Note => ({ source: `plugin:${p.manifest.id}`, level: "info", text: `${p.manifest.name} saw the response` })),
      ];
    },
  };
  return api;
}

/** `imports.analyzeWithPlugin` of the mock: which plugin importer, checked like the real one. */
export function mockPluginImporter(plugins: PluginApi, plugin: string, importer: string): Promise<{ name: string }> {
  return (async () => {
    const p = (await plugins.list()).find((x) => x.manifest.id === plugin);
    if (!p) throw new BackendError("plugin", `plugin ${plugin} is not installed`);
    if (!p.enabled) throw new BackendError("plugin", `plugin ${plugin} is disabled`);
    const imp = p.manifest.contributes.importers.find((i) => i.id === importer);
    if (!imp) throw new BackendError("plugin", `plugin ${plugin} has no importer ${importer}`);
    return { name: `${p.manifest.name} import` };
  })();
}
