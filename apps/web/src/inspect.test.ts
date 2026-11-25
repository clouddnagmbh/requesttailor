// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's inspect API (wave 18, strand B) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): a token read in the browser's core, the tokens of a resolved
// request, a rule switched off for the collection (staged, unsaved) and on this device
// (localStorage). Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { DEVICE_KEY, deviceIgnored, webInspect, type CoreInspectModule, type CoreInspectWorkspace } from "./inspect";
import type { CoreModule } from "./wasm";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

type Ws = CoreInspectWorkspace & { dirty(): string[]; free(): void };
type Mod = CoreInspectModule & { Workspace: { open(files: Record<string, string>): Ws } };

async function load(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as Mod & {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod;
}

const b64 = (v: unknown) => Buffer.from(JSON.stringify(v)).toString("base64url");
const jwt = (claims: Record<string, unknown>) => `${b64({ alg: "RS256" })}.${b64(claims)}.c2ln`;
const now = () => Math.floor(Date.now() / 1000);

const FILES = {
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n',
  "a.yml":
    'info:\n  name: A\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "https://api.test/a?access_token={{tok}}"\n  auth: inherit\n',
};

function storage(): Storage {
  const m = new Map<string, string>();
  return {
    getItem: (k) => m.get(k) ?? null,
    setItem: (k, v) => void m.set(k, v),
    removeItem: (k) => void m.delete(k),
    clear: () => m.clear(),
    key: () => null,
    length: 0,
  };
}

beforeEach(() => void vi.stubGlobal("localStorage", storage()));
afterEach(() => void vi.unstubAllGlobals());

describe.skipIf(!built)("webInspect over the real core", () => {
  async function api() {
    const mod = await load();
    const ws = mod.Workspace.open(FILES);
    const host = {
      session: () => ({ ws }) as unknown as Session,
      core: async () => mod as unknown as CoreModule,
      coreInput: async (input: { environment: string | null; vars: [string, string][] }) => ({ ...input, secrets: [] as [string, string][] }),
    };
    return { api: webInspect(host), ws };
  }

  it("reads a token, never verifies it, and explains a broken one", async () => {
    const { api: inspect } = await api();
    const info = await inspect.jwt(jwt({ iss: "https://idp.example", exp: now() - 720 }));
    expect(info.signature_verified).toBe(false);
    expect(info.claims[0]!.name).toBe("iss");
    expect(info.warnings.map((w) => w.kind)).toEqual(["expired"]);
    await expect(inspect.jwt("nope")).rejects.toMatchObject({ kind: "shell" });
  });

  it("finds the token of the resolved request, with the variable filled in", async () => {
    const { api: inspect, ws } = await api();
    // The variable is not set: no token.
    expect(await inspect.tokens({ path: "a.yml", environment: null })).toEqual([]);
    const input = { environment: null, vars: [["tok", jwt({ exp: now() + 3600 })]] as [string, string][], secrets: [] };
    const found = ws.inspectTokens("a.yml", input, null);
    expect(found).toHaveLength(1);
    expect(found[0]!.location).toEqual({ kind: "query" });
    expect(found[0]!.token.warnings).toEqual([]);
  });

  it("switches a rule off for the collection (staged) and on this device (a list of ids)", async () => {
    const { api: inspect, ws } = await api();
    await inspect.ignoreCheck("chk/hsts-missing", "collection");
    expect(ws.inspectCollectionIgnored()).toEqual(["chk/hsts-missing"]);
    expect(ws.dirty()).toEqual(["opencollection.yml"]);
    await inspect.ignoreCheck("chk/stack-trace", "device");
    await inspect.ignoreCheck("chk/stack-trace", "device");
    expect(deviceIgnored()).toEqual(["chk/stack-trace"]);
    expect(JSON.parse(localStorage.getItem(DEVICE_KEY)!)).toEqual(["chk/stack-trace"]);
    await expect(inspect.ignoreCheck("chk/nope", "collection")).rejects.toMatchObject({ kind: "shell" });
    // Without a response there is nothing to check.
    expect(await inspect.checks({ path: "a.yml", environment: null })).toEqual([]);
  });
});

describe("the device list", () => {
  it("reads only a list of text and survives a broken storage", () => {
    localStorage.setItem(DEVICE_KEY, JSON.stringify(["a", 1, "b"]));
    expect(deviceIgnored()).toEqual(["a", "b"]);
    localStorage.setItem(DEVICE_KEY, "{broken");
    expect(deviceIgnored()).toEqual([]);
    vi.stubGlobal("localStorage", undefined);
    expect(deviceIgnored()).toEqual([]);
  });
});
