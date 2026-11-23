// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The OpenAPI wizard over the real core (rt-wasm, built with `node scripts/build-wasm.mjs`):
// inspect, plan, write into an open collection (the spec staged and named the contract spec),
// then the run wizard's requirements and the response check read the staged spec. Skipped
// when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import type { FileChange, FolderImport, ImportPlan, SendRequirements, SpecOverview } from "@requesttailor/ui";

import { looksBlocked } from "./openapi";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

const SPEC = JSON.stringify({
  openapi: "3.0.3",
  info: { title: "Pets", version: "1.0.0" },
  servers: [{ url: "https://pets.example.com/v1", description: "Production" }],
  paths: {
    "/pets/{petId}": {
      get: {
        tags: ["pets"],
        summary: "Get a pet",
        parameters: [{ name: "petId", in: "path", required: true, schema: { type: "integer", minimum: 1 } }],
        responses: { "200": { description: "ok" } },
      },
    },
    "/users": { get: { tags: ["users"], summary: "List users", responses: { "200": { description: "ok" } } } },
  },
});

interface Core {
  initSync(options: { module: Uint8Array }): unknown;
  openapiInspect(files: { name: string; data: Uint8Array }[]): SpecOverview;
  Importer: new () => {
    openapiPlan(files: { name: string; data: Uint8Array }[], options: unknown): ImportPlan;
    take(id: number): FileChange[];
  };
  Workspace: {
    open(files: Map<string, string>): {
      addImport(files: FileChange[], name: string): FolderImport;
      openapiSpecPath(): string | null | undefined;
      openapiRequirements(path: string, input: unknown, specs: [string, Uint8Array][]): SendRequirements | null | undefined;
      tree(): { kind: string; path: string; children?: unknown[] };
    };
  };
}

async function core(): Promise<Core> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as Core;
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod;
}

const files = [{ name: "pets.json", data: new TextEncoder().encode(SPEC) }];
const OPTIONS = {
  operations: ["GET /pets/{petId}"],
  grouping: "flat",
  naming: "summary",
  examples: "spec",
  servers: [{ index: 0, environment: "Production" }],
  default_server: 0,
  collection_name: null,
  keep_spec: true,
};

function requests(node: { kind: string; path: string; children?: unknown[] }): string[] {
  if (node.kind === "request") return [node.path];
  return (node.children ?? []).flatMap((c) => requests(c as typeof node));
}

describe("CORS detection", () => {
  it("tells a browser refusal from an HTTP error", () => {
    expect(looksBlocked("cannot fetch https://x/openapi.json: Failed to fetch")).toBe(true);
    expect(looksBlocked("cannot fetch https://x/openapi.json: the request was blocked by the browser")).toBe(true);
    expect(looksBlocked("cannot fetch https://x/openapi.json: HTTP 404")).toBe(false);
  });
});

describe.skipIf(!built)("OpenAPI wizard over rt-wasm", () => {
  it("inspects, imports into the open collection and asks for the missing path parameter", async () => {
    const c = await core();
    const overview = c.openapiInspect(files);
    expect(overview.title).toBe("Pets");
    expect(overview.operations.map((o) => o.key)).toEqual(["GET /pets/{petId}", "GET /users"]);

    const importer = new c.Importer();
    const plan = importer.openapiPlan(files, OPTIONS);
    expect(plan.report.requests).toBe(1);

    const ws = c.Workspace.open(new Map([["opencollection.yml", "opencollection: 1.0.0\ninfo:\n  name: Shop\n"]]));
    const added = ws.addImport(importer.take(plan.id), "Pets");
    expect(added.contract_spec).toMatch(/^Pets\/openapi\.json$/);
    expect(ws.openapiSpecPath()).toBe(added.contract_spec);

    const [path] = requests(ws.tree());
    // The spec is staged in the workspace: nothing to read from disk.
    const req = ws.openapiRequirements(path!, { environment: "Production", vars: [], secrets: [] }, []);
    expect(req?.operation).toBe("GET /pets/{petId}");
    expect(req?.ready).toBe(false);
    expect(req?.fields.find((f) => f.id === "path:petId")?.problem?.kind).toBe("missing");
  });
});
