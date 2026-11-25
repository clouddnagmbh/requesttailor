// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's minimal-case API (wave 19, strand E) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`) and a real local server: the cause stays, the rest goes, the
// secret value of the environment goes out on the wire and nowhere else, a writing request asks
// first, and a failure that does not show is told. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { webShrink, type CoreShrinkWorkspace } from "./shrink";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const CANARY = "RT-CANARY-web-shrink-31ab";

type Ws = CoreShrinkWorkspace & { free(): void };

async function open(files: Record<string, string>): Promise<Ws> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
    Workspace: { open(files: Record<string, string>): Ws };
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod.Workspace.open(files);
}

let server: Server;
let base = "";
const seen: string[] = [];

beforeAll(async () => {
  server = createServer((req, res) => {
    seen.push(`${req.method} ${req.url}`);
    const fails = (req.url ?? "").includes("boom=1");
    res.writeHead(fails ? 500 : 200, { "content-type": "application/json", "access-control-allow-origin": "*" });
    res.end(fails ? '{"error":"boom"}' : "{}");
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
  server?.close();
});

const files = () => ({
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n',
  "environments/dev.yml": `name: dev\nvariables:\n  - name: base\n    value: ${base}\n  - name: token\n    secret: true\n`,
  "get.yml": `info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "{{base}}/items?a=1&boom=1&c=3"\n  headers:\n    - name: X-A\n      value: one\n    - name: Authorization\n      value: Bearer {{token}}\n`,
  "post.yml": `info:\n  name: Post\n  type: http\n  seq: 2\n\nhttp:\n  method: POST\n  url: "{{base}}/items?boom=1&z=1"\n`,
});

function api(ws: Ws) {
  const session = () => ({ ws }) as unknown as Session;
  const sendContext = async () => ({
    input: { environment: "dev", vars: [] as [string, string][], secrets: [["token", CANARY]] as [string, string][] },
    companion: null,
    services: null,
  });
  return webShrink({ session, sendContext });
}

describe.skipIf(!built)("webShrink over the real core", () => {
  it("keeps the cause, takes the rest away and never shows the secret", async () => {
    const ws = await open(files());
    const r = await api(ws).run({ path: "get.yml", environment: "dev", predicate: { kind: "server-error" } });
    expect(r.request.url).toBe("{{base}}/items?boom=1");
    expect(r.request.headers.map((h) => h.name)).toEqual(["Authorization"]);
    expect(r.removed.map((s) => s.part).sort()).toEqual(["header:X-A", "query:a", "query:c"]);
    expect(r.one_minimal).toBe(true);
    expect(JSON.stringify(r)).not.toContain(CANARY);
    expect(seen.length).toBe(r.probes);
    ws.free();
  });

  it("asks before writes and says when the failure does not show", async () => {
    const ws = await open(files());
    const shrink = api(ws);
    await expect(shrink.run({ path: "post.yml", environment: "dev", predicate: { kind: "server-error" } })).rejects.toThrow(/requests that write/);
    const ok = await shrink.run({ path: "post.yml", environment: "dev", predicate: { kind: "server-error" }, confirmed_writes: true });
    expect(ok.request.url).toBe("{{base}}/items?boom=1");
    const none = await shrink.run({ path: "get.yml", environment: "dev", predicate: { kind: "status", status: 404 } });
    expect(none.not_reproduced).toBe(true);
    await expect(shrink.predicate("get.yml", null)).rejects.toThrow(/no answer yet/);
    ws.free();
  });
});
