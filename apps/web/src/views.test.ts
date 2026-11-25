// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's views API (wave 18, strand G) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): the file `<request>.views.json` is staged unsaved next to the
// request with the bytes the core writes, names are unique, jq is said not to be available.
// Evaluating on a kept response is covered by `crates/rt-app/tests/views.rs`. Skipped when the
// core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { webViews, type CoreViewsWorkspace } from "./views";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

type Ws = CoreViewsWorkspace & {
  dirty(): string[];
  free(): void;
};

async function open(files: Record<string, string>): Promise<Ws> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
    Workspace: { open(files: Record<string, string>): Ws };
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod.Workspace.open(files);
}

const FILES = {
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n',
  "users/get.yml":
    'info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "http://127.0.0.1/x"\n  auth: inherit\n',
};

describe.skipIf(!built)("webViews over the real core", () => {
  it("stages the file next to the request and keeps names unique", async () => {
    const ws = await open(FILES);
    const api = webViews(() => ({ ws }) as unknown as Session);
    expect(await api.list("users/get.yml")).toEqual([]);
    expect(ws.dirty()).toEqual([]);
    await api.save("users/get.yml", { name: "Ids", language: "jsonpath", expression: "$.items[*].id", display: "json" });
    await api.save("users/get.yml", {
      name: "Users",
      language: "jsonpath",
      expression: "$.items",
      display: "table",
      columns: ["id", "name"],
    });
    expect(ws.dirty()).toEqual(["users/get.views.json"]);
    const again = await api.save("users/get.yml", {
      name: "Ids",
      language: "jsonpath",
      expression: "$.items[*].name",
      display: "text",
    });
    expect(again.map((v) => v.name)).toEqual(["Ids", "Users"]);
    expect(again[0]?.expression).toBe("$.items[*].name");
    expect((await api.remove("users/get.yml", "Ids")).map((v) => v.name)).toEqual(["Users"]);
    expect(await api.languages()).toEqual(["jsonpath"]);
    ws.free();
  });

  it("refuses what the core refuses", async () => {
    const ws = await open(FILES);
    const api = webViews(() => ({ ws }) as unknown as Session);
    await expect(
      api.save("users/get.yml", { name: "J", language: "jq", expression: ".a", display: "json" }),
    ).rejects.toThrow(/jq is not available in this build/);
    await expect(
      api.save("users/get.yml", { name: "X", language: "jsonpath", expression: "$.[", display: "json" }),
    ).rejects.toThrow(/JSONPath/);
    await expect(
      api.save("nope.yml", { name: "X", language: "jsonpath", expression: "$", display: "json" }),
    ).rejects.toThrow();
    expect(ws.dirty()).toEqual([]);
    ws.free();
  });
});
