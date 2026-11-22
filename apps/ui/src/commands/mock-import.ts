// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Import in the mock backend, for the import UI's tests. Owner: wave 4, strand B.
//
// Detects the format from names and content like the core does (roughly), and answers with a
// fixed, realistic plan per format. Files whose text contains `INVALID` are refused, so tests
// can show the error path. `write` "opens" the mock's demo collection.

import { BackendError, type CollectionInfo, type ImportApi, type ImportSource, type PluginApi } from "./backend";
import { mockPluginImporter } from "./mock-plugins";
import type { ImportFormat, ImportPlan, Loss } from "./types";

const text = (f: ImportSource) => new TextDecoder().decode(f.data);

function detect(f: ImportSource): Exclude<ImportFormat, "plugin"> | null {
  const name = f.name.toLowerCase();
  const t = text(f).trimStart();
  if (name.endsWith(".http") || name.endsWith(".rest") || name.startsWith("http-client")) return "http";
  if (t.startsWith("curl ")) return "curl";
  if (name.endsWith(".har") || t.includes('"log"')) return "har";
  if (t.includes("schema.getpostman.com") || name.includes("postman")) return "postman";
  if (t.includes("__export_format") || t.includes("insomnia.rest")) return "insomnia";
  if (/^\s*["']?openapi["']?\s*:/m.test(t) || t.includes('"openapi"')) return "openapi";
  if (/^(GET|POST|PUT|PATCH|DELETE) /m.test(t) || t.includes("###")) return "http";
  return null;
}

const LOSSES: Record<Exclude<ImportFormat, "plugin">, Loss[]> = {
  postman: [
    { location: "Users", kind: "script", effect: "dropped", detail: "pre-request script (3 lines) not imported" },
    { location: "Users / Get user", kind: "test", effect: "dropped", detail: "test script (5 lines) not imported" },
    {
      location: "Users / Login",
      kind: "auth",
      effect: "approximated",
      detail:
        "the password is stored in the request file as typed; consider a secret variable (set its value in Environments) and use {{name}} here",
    },
    {
      location: "environment Production",
      kind: "variable",
      effect: "dropped",
      detail: "secret value of `token` not imported; set it in Environments",
    },
  ],
  insomnia: [
    {
      location: "Orders / Create",
      kind: "variable",
      effect: "approximated",
      detail: "template tag `{% uuid %}` kept as text; RequestTailor does not evaluate it",
    },
  ],
  openapi: [
    {
      location: "collection",
      kind: "documentation",
      effect: "dropped",
      detail: "schemas and response definitions are API documentation and not imported; explicit response examples are kept as request examples",
    },
    {
      location: "Pets / Create pet",
      kind: "body",
      effect: "approximated",
      detail: "request body generated from the schema; edit the example values",
    },
  ],
  curl: [],
  har: [
    {
      location: "GET shop.example.com/products",
      kind: "auth",
      effect: "approximated",
      detail:
        "the credential header is stored in the request file as typed; consider a secret variable (set its value in Environments) and use {{name}} here",
    },
  ],
  http: [
    { location: "login", kind: "script", effect: "dropped", detail: "response handler script (2 lines) not imported" },
  ],
};

const COUNTS: Record<Exclude<ImportFormat, "plugin">, [number, number, number]> = {
  postman: [4, 1, 1],
  insomnia: [3, 1, 2],
  openapi: [6, 2, 1],
  curl: [1, 0, 0],
  har: [5, 0, 0],
  http: [2, 0, 1],
};

function stem(name: string): string {
  const base = name.split(/[\\/]/).pop() ?? name;
  return base.replace(/\.(postman_collection\.)?(json|ya?ml|har|http|rest|sh|txt)$/i, "") || "Imported collection";
}

export function mockImports(
  open?: () => Promise<CollectionInfo | null>,
  plugins?: () => PluginApi,
): ImportApi {
  let nextId = 1;
  const plans = new Map<number, ImportPlan>();
  return {
    analyze: async (files, format) => {
      if (files.length === 0) throw new BackendError("import", "no file to import");
      const bad = files.find((f) => text(f).includes("INVALID"));
      if (bad) throw new BackendError("import", `${bad.name} is not valid JSON: line 1, column 2: expected a value`);
      const detected = format === "plugin" ? null : (format ?? detect(files[0]!));
      if (!detected) throw new BackendError("import", "unknown file format; choose the format explicitly");
      const [requests, folders, environments] = COUNTS[detected];
      const name = detected === "curl" ? "api.example.com" : stem(files[0]!.name);
      const plan: ImportPlan = {
        id: nextId++,
        format: detected,
        collection_name: name,
        folder_name: name,
        files: requests + folders + environments + 1,
        report: {
          format: detected,
          requests,
          folders,
          environments: environments + Math.max(0, files.length - 1),
          losses: LOSSES[detected].map((l) => ({ ...l })),
        },
      };
      plans.set(plan.id, plan);
      return plan;
    },
    write: async (id) => {
      const plan = plans.get(id);
      if (!plan) throw new BackendError("import", `import ${id} not found`);
      plans.delete(id);
      const info = (await open?.()) ?? { id: "mock-collection", name: plan.collection_name, location: "mock://import" };
      return { ...info, name: plan.collection_name, location: `mock://${plan.folder_name}` };
    },
    discard: async (id) => {
      plans.delete(id);
    },
    // Wave 5, strand A: plugin importers of the mock plugins (one request per file).
    analyzeWithPlugin: async (files, plugin, importer) => {
      if (!plugins) throw new BackendError("plugin", `plugin ${plugin} is not installed`);
      if (files.length === 0) throw new BackendError("import", "no file to import");
      const { name } = await mockPluginImporter(plugins(), plugin, importer);
      const plan: ImportPlan = {
        id: nextId++,
        format: "plugin",
        collection_name: name,
        folder_name: name,
        files: files.length + 1,
        report: { format: "plugin", requests: files.length, folders: 0, environments: 0, losses: [] },
      };
      plans.set(plan.id, plan);
      return plan;
    },
  };
}
