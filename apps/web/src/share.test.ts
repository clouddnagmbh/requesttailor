// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's share API (wave 17, strand C) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): a link made from the request in memory, read back, no secret
// in it, snippets that keep variables as variables, and not one network request on the way.
// Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { afterEach, describe, expect, it, vi } from "vitest";

import { ownBase, webShare, type CoreShare, type CoreShareWorkspace } from "./share";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

type Ws = CoreShareWorkspace & {
  updateRequest(path: string, spec: unknown): void;
  request(path: string): Record<string, unknown>;
};
type Mod = CoreShare & { Workspace: { open(files: Record<string, string>): Ws } };

async function core(): Promise<Mod> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as Mod & {
    initSync(options: { module: Uint8Array }): unknown;
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod;
}

const CANARY = "RT-CANARY-web-share-1";
const FILES = {
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Shop\n',
  "environments/Dev.yml": `name: Dev\nvariables:\n  - name: baseUrl\n    value: https://api.dev.test\n  - name: token\n    value: ''\n    secret: true\n`,
  "items.yml": `info:\n  name: Items\n  type: http\n  seq: 1\n\nhttp:\n  method: POST\n  url: "{{baseUrl}}/items?api_key=${CANARY}"\n  headers:\n    - name: Authorization\n      value: "Bearer {{token}}"\n    - name: X-Api-Key\n      value: ${CANARY}\n  body:\n    type: json\n    data: '{"password": "${CANARY}", "name": "x"}'\n`,
};

afterEach(() => vi.unstubAllGlobals());

describe.skipIf(!built)("webShare over the real core", () => {
  it("makes a link from the request in memory, without a request and without a secret", async () => {
    const mod = await core();
    const ws = mod.Workspace.open(FILES);
    const fetched = vi.fn(() => Promise.reject(new Error("no network expected")));
    vi.stubGlobal("fetch", fetched);
    const api = webShare(() => ({ ws }) as unknown as Session, async () => mod, () => "https://app.example.test/");

    const link = await api.link("items.yml", { include_values: true, environment: "Dev" });
    expect(link.text.startsWith("https://app.example.test/#rt=1.")).toBe(true);
    expect(link.text).not.toContain(CANARY);
    expect(link.preview).not.toContain(CANARY);
    expect(link.preview).toContain("{{secret:api_key}}");
    expect(link.payload.variables.find((v) => v.name === "baseUrl")?.value).toBe("https://api.dev.test");
    expect(link.payload.variables.find((v) => v.name === "token")).toMatchObject({ secret: true, value: null });

    // The same request comes back from the text, whichever way it is pasted.
    const fragment = link.text.slice(link.text.indexOf("#"));
    for (const text of [link.text, fragment, `rt:${fragment.slice("#rt=".length)}`]) {
      expect((await api.open(text)).request.url).toBe("{{baseUrl}}/items?api_key={{secret:api_key}}");
    }
    expect(await api.qr(link.text)).toContain("<svg");
    expect(fetched).not.toHaveBeenCalled();
  });

  it("reads edits that are not saved, and refuses what is no link", async () => {
    const mod = await core();
    const ws = mod.Workspace.open(FILES);
    const api = webShare(() => ({ ws }) as unknown as Session, async () => mod, () => "https://app.example.test/");
    const spec = ws.request("items.yml");
    ws.updateRequest("items.yml", { ...spec, name: "Edited", url: "{{baseUrl}}/edited" });
    const link = await api.link("items.yml", {});
    expect(link.payload.request.url).toBe("{{baseUrl}}/edited");
    expect(link.payload.name).toBe("Edited");
    await expect(api.open("https://x.test/#nothing")).rejects.toMatchObject({ kind: "format" });
    await expect(api.link("nope.yml", {})).rejects.toMatchObject({ kind: "request-not-found" });
    await expect(api.link("items.yml", { environment: "Nope" })).rejects.toMatchObject({ kind: "environment-not-found" });
  });

  it("copies as curl, fetch and Python with variables kept as variables", async () => {
    const mod = await core();
    const ws = mod.Workspace.open(FILES);
    const api = webShare(() => ({ ws }) as unknown as Session, async () => mod);
    const curl = await api.copyAs("items.yml", "curl");
    expect(curl.text).toContain("$BASE_URL/items");
    expect(curl.text).toContain("Bearer $TOKEN");
    const fetchText = (await api.copyAs("items.yml", "fetch")).text;
    expect(fetchText).toContain("process.env.TOKEN");
    const py = (await api.copyAs("items.yml", "python")).text;
    expect(py).toContain('os.environ["TOKEN"]');
    for (const t of [curl.text, fetchText, py]) expect(t).not.toContain(CANARY);
    const resolved = await api.copyAs("items.yml", "curl", { resolve_variables: true, environment: "Dev" });
    expect(resolved.text).toContain("https://api.dev.test/items");
    expect(resolved.text).toContain("$TOKEN");
  });
});

describe("ownBase", () => {
  it("is the app's address without query and fragment", () => {
    expect(ownBase({ origin: "https://app.example.test", pathname: "/rt/" })).toBe("https://app.example.test/rt/");
  });
});
