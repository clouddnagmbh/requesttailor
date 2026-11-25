// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's consumer contracts (wave 19, strand F) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`), in-memory folders and a real local server: the consumers listed
// in the collection's `opencollection.yml` are read from the collection's own repository, one
// that breaks is named with its request and assertion, a consumer outside the repository or at a
// Git URL is reported (never skipped, never fetched), and no personal value is in the result.
// Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { inside, webConsumers } from "./consumers";
import { fill, memoryFolder } from "./git/fake-handle.test-util";
import type { CoreModule } from "./wasm";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const PERSONAL = "frank.canary@example.test";

let core: CoreModule;
let server: Server;
let base = "";
let changed = false;
const hits: string[] = [];

beforeAll(async () => {
  if (!built) return;
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as CoreModule & {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  core = mod;
  server = createServer((req, res) => {
    hits.push(`${req.method} ${req.url}`);
    const cors = { "content-type": "application/json", "access-control-allow-origin": "*", "access-control-allow-headers": "*" };
    if (req.method === "OPTIONS") {
      res.writeHead(204, cors);
      res.end();
      return;
    }
    res.writeHead(200, cors);
    res.end(JSON.stringify({ items: [{ price: changed ? `${PERSONAL} 12` : 12.5 }] }));
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
  server?.close();
});

const manifest = (name: string, extra = "") => `opencollection: "1.0.0"\ninfo:\n  name: ${name}\n${extra}`;
const consumers = (entries: string) => `extensions:\n  requesttailor:\n    consumers:\n${entries}`;

async function repo(entries: string) {
  const root = memoryFolder("repo");
  await fill(root, {
    "provider/opencollection.yml": manifest("provider", consumers(entries)),
    "billing/opencollection.yml": manifest("billing"),
    "billing/environments/ci.yml": "name: ci\nvariables:\n  - name: baseUrl\n    value: http://localhost:1\n",
    "billing/get.yml":
      'info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "{{baseUrl}}/orders/1"\n\nruntime:\n  assertions:\n    - expression: res.body.items[0].price\n      operator: isNumber\n',
    "billing/post.yml": 'info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: "{{baseUrl}}/orders"\n',
  });
  return root;
}

function api(root: FileSystemDirectoryHandle) {
  return webConsumers({
    repository: () => ({ handle: root, scope: "provider" }),
    core: async () => core,
    sendContext: async () => ({ input: { environment: null, vars: [], secrets: [] }, companion: null, services: null }),
  });
}

const BILLING = "      - name: billing\n        source: ../billing\n        environment: ci\n";

describe("inside", () => {
  it("resolves against the collection's folder and refuses to leave the repository", () => {
    expect(inside("provider", "../billing")).toEqual(["billing"]);
    expect(inside("services/orders", "../../billing", "tests")).toEqual(["billing", "tests"]);
    expect(inside("provider", "../../elsewhere")).toBeNull();
    expect(inside("", "..")).toBeNull();
    expect(inside("provider", "./local")).toEqual(["provider", "local"]);
  });
});

describe.skipIf(!built)("the web consumers", () => {
  it("lists what opencollection.yml names", async () => {
    const list = await api(await repo(BILLING)).list();
    expect(list.map((c) => c.name)).toEqual(["billing"]);
    expect(list[0]!.environment).toBe("ci");
  });

  it("names the breaking consumer and request and leaves writes out", async () => {
    changed = true;
    hits.length = 0;
    const report = await api(await repo(BILLING)).verify({ base_url: base });
    const billing = report.consumers[0]!;
    expect(billing.failures).toHaveLength(1);
    expect(billing.failures[0]).toMatchObject({ request: "GET /orders/1", request_path: "get.yml" });
    expect(billing.failures[0]!.difference).toContain("got string");
    expect(billing.skipped).toBe(1);
    expect(hits.some((h) => h.startsWith("POST"))).toBe(false);
    expect(JSON.stringify(report)).not.toContain(PERSONAL);
    changed = false;
    const green = await api(await repo(BILLING)).verify({ base_url: base, allow_writes: true });
    expect(green.consumers[0]!.failures).toEqual([]);
    expect(green.consumers[0]!.requests).toBe(2);
  });

  it("reports a consumer outside the repository or at a Git URL, and fetches nothing", async () => {
    hits.length = 0;
    const entries = `${BILLING}      - name: far\n        source: ../../elsewhere\n        environment: ci\n      - name: remote\n        source: https://git.example.test/a/b.git\n        environment: ci\n`;
    const report = await api(await repo(entries)).verify({ base_url: base });
    expect(report.consumers.map((c) => c.name)).toEqual(["billing", "far", "remote"]);
    expect(report.consumers[1]!.error).toContain("inside this collection's repository");
    expect(report.consumers[2]!.error).toContain("inside this collection's repository");
    expect(hits.every((h) => h.includes("/orders"))).toBe(true);
  });

  it("refuses a name that is not listed", async () => {
    await expect(api(await repo(BILLING)).verify({ base_url: base, only: ["nobody"] })).rejects.toThrow(/nobody/);
  });
});
