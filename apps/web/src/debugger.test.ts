// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's script debugger (wave 19, strand Y) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): a breakpoint, a step, a variable, an expression, a masked
// secret, abort. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { webDebugger } from "./debugger";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));
const SECRET = "RT-CANARY-web-debug-secret";

const CODE = "const who = 'web';\\nconst n = 6;\\nrt.request.headers.set('X-N', who + n);\\nconsole.log(rt.vars.get('token'));\\n";
const FILES = {
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n',
  "environments/Local.yml": "name: Local\nvariables:\n  - secret: true\n    name: token\n",
  "get.yml": `info:\n  name: Get\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: http://x.test/\n\nruntime:\n  scripts:\n    - type: before-request\n      code: "${CODE}"\n`,
};

async function open() {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
    Workspace: { open(files: Record<string, string>): unknown };
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod.Workspace.open(FILES);
}

describe.skipIf(!built)("webDebugger over the real core", { timeout: 120_000 }, () => {
  it("stops, steps, reads variables, answers an expression and masks the secret", async () => {
    const ws = await open();
    const api = webDebugger({
      session: () => ({ id: "c", ws }) as unknown as Session,
      trusted: () => true,
      input: async () => ({ environment: "Local", vars: [], secrets: [["token", SECRET]] }),
    });
    const first = await api.start({ request_path: "get.yml", phase: "before-request", environment: "Local", breakpoints: [{ line: 3 }] });
    expect(first.status).toBe("paused");
    expect(first.pause?.line).toBe(3);
    const who = first.pause?.scopes.flatMap((s) => s.values).find((v) => v.name === "who");
    expect(who?.preview).toBe('"web"');
    const asked = await api.command(first.session, { kind: "evaluate", expression: "n * 7" });
    expect(asked.pause?.evaluated?.preview).toBe("42");
    const secret = await api.command(first.session, { kind: "evaluate", expression: "rt.vars.get('token')" });
    expect(JSON.stringify(secret)).not.toContain(SECRET);
    const stepped = await api.command(first.session, { kind: "step-over" });
    expect(stepped.pause?.line).toBe(4);
    const done = await api.command(first.session, { kind: "continue" });
    expect(done.status).toBe("finished");
    expect(JSON.stringify(done)).not.toContain(SECRET);
    await api.stop(first.session);
  });

  it("refuses an untrusted collection and aborts on request", async () => {
    const ws = await open();
    const base = {
      session: () => ({ id: "c", ws }) as unknown as Session,
      input: async () => ({ environment: null, vars: [], secrets: [] }),
    };
    const untrusted = webDebugger({ ...base, trusted: () => false });
    await expect(untrusted.start({ request_path: "get.yml", phase: "before-request", breakpoints: [] })).rejects.toThrow(/Trust/);
    const api = webDebugger({ ...base, trusted: () => true });
    const s = await api.start({ request_path: "get.yml", phase: "before-request", breakpoints: [], stop_on_entry: true });
    expect(s.status).toBe("paused");
    expect((await api.command(s.session, { kind: "abort" })).status).toBe("aborted");
  });
});
