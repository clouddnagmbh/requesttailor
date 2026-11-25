// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's access check (wave 19, strand D) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`) and a real local server with owners: user B reads A's order
// where the server leaks, is denied where it does not, each request carries its own user's
// secret, and neither a secret nor a personal value is in the result. A writing request needs
// permission. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { join } from "node:path";

import { afterAll, beforeAll, describe, expect, it } from "vitest";

import { webAccess, type CoreAccessWorkspace } from "./access";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const SECRET_A = "RT-CANARY-web-access-A-62d1";
const SECRET_B = "RT-CANARY-web-access-B-09f7";
const PERSONAL = "dave.canary@example.test";

type Ws = CoreAccessWorkspace & { free(): void };

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
let leak = true;
const seen: { url: string; user: string; auth: string }[] = [];

beforeAll(async () => {
  server = createServer((req, res) => {
    const user = String(req.headers["x-user"] ?? "");
    seen.push({ url: req.url ?? "", user, auth: String(req.headers.authorization ?? "") });
    const cors = { "content-type": "application/json", "access-control-allow-origin": "*", "access-control-allow-headers": "*" };
    if (req.method === "OPTIONS") {
      res.writeHead(204, cors);
      res.end();
      return;
    }
    const mine = user === "alice";
    const ok = req.url === "/orders/1" && (mine || leak);
    res.writeHead(ok ? 200 : 403, cors);
    res.end(ok ? JSON.stringify({ id: 1, owner: "alice", email: PERSONAL }) : '{"error":"forbidden"}');
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
  server?.close();
});

const env = (name: string, user: string) =>
  `name: ${name}\nvariables:\n  - name: base\n    value: ${base}\n  - name: user\n    value: ${user}\n  - name: orderId\n    value: "1"\n  - name: token\n    secret: true\n`;

const headers = "  headers:\n    - name: x-user\n      value: \"{{user}}\"\n    - name: Authorization\n      value: Bearer {{token}}\n";

const files = () => ({
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n',
  "environments/alice.yml": env("alice", "alice"),
  "environments/bob.yml": env("bob", "bob"),
  "get.yml": `info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "{{base}}/orders/{{orderId}}"\n${headers}`,
  "put.yml": `info:\n  name: Put\n  type: http\n  seq: 2\n\nhttp:\n  method: PUT\n  url: "{{base}}/orders/{{orderId}}"\n${headers}`,
});

function api(ws: Ws) {
  const session = () => ({ ws }) as unknown as Session;
  const sendContext = async (input: { environment?: string | null }) => ({
    input: {
      environment: input.environment ?? null,
      vars: [] as [string, string][],
      secrets: [["token", input.environment === "alice" ? SECRET_A : SECRET_B]] as [string, string][],
    },
    companion: null,
    services: null,
  });
  return webAccess({ session, sendContext });
}

const AB = { a: "alice", b: "bob" };

describe.skipIf(!built)("webAccess over the real core", () => {
  it("finds a leak, sends each user's own secret and shows no secret or personal value", async () => {
    leak = true;
    seen.length = 0;
    const ws = await open(files());
    const r = await api(ws).check("get.yml", AB, { rate: 1000 });
    expect(r.verdict.kind).toBe("broken-object-access");
    expect(r.status_a).toBe(200);
    expect(r.status_b).toBe(200);
    const orders = seen.filter((s) => s.url === "/orders/1");
    expect(orders.map((s) => [s.user, s.auth])).toEqual([
      ["alice", `Bearer ${SECRET_A}`],
      ["bob", `Bearer ${SECRET_B}`],
    ]);
    const all = JSON.stringify(r);
    for (const c of [SECRET_A, SECRET_B, PERSONAL]) expect(all).not.toContain(c);
    ws.free();
  });

  it("is ok where B is denied, and writes need permission", async () => {
    leak = false;
    const ws = await open(files());
    const access = api(ws);
    const ok = await access.check("get.yml", AB, { rate: 1000 });
    expect(ok.verdict.kind).toBe("ok");
    expect(ok.status_b).toBe(403);
    seen.length = 0;
    const put = await access.check("put.yml", AB, { rate: 1000 });
    expect(put.verdict).toEqual({ kind: "skipped", reason: "skipped (writes)" });
    expect(seen).toHaveLength(0);
    await expect(access.check("get.yml", { a: "alice", b: "alice" }, {})).rejects.toThrow();
    ws.free();
  });
});
