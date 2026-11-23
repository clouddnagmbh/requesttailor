// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's folder-scripts API (wave 16) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): read, replace unsaved, read-back check, unknown folder.
// Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import type { Script } from "@requesttailor/ui";

import { webFolderScripts, type CoreFolderScriptsWorkspace } from "./folder-scripts";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

type Ws = CoreFolderScriptsWorkspace & { dirty(): string[]; free(): void };

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
  "users/folder.yml": "info:\n  name: Users\n  type: folder\n  seq: 1\n",
  "users/get.yml": "info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://x.test/\n",
};

describe.skipIf(!built)("webFolderScripts over the real core", () => {
  it("reads and replaces scripts of the collection and a folder, unsaved", async () => {
    const ws = await open(FILES);
    const api = webFolderScripts(() => ({ ws }) as unknown as Session);
    const scripts: Script[] = [{ phase: "before-request", code: "rt.vars.set('a', 1);\n" }];
    expect(await api.get("")).toEqual([]);
    expect(await api.get("users")).toEqual([]);
    await api.set("users", scripts);
    await api.set("", scripts);
    expect(await api.get("users")).toEqual(scripts);
    expect(await api.get("")).toEqual(scripts);
    expect(ws.dirty().sort()).toEqual(["opencollection.yml", "users/folder.yml"]);
    // Back to what was saved: clean again.
    await api.set("users", []);
    await api.set("", []);
    expect(ws.dirty()).toEqual([]);
  });

  it("refuses an unknown folder", async () => {
    const ws = await open(FILES);
    const api = webFolderScripts(() => ({ ws }) as unknown as Session);
    await expect(api.get("nope")).rejects.toMatchObject({ kind: "folder-not-found" });
    await expect(api.set("nope", [])).rejects.toMatchObject({ kind: "folder-not-found" });
  });
});
