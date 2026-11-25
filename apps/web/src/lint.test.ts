// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's lint API (wave 18, strand F) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): the four rules and the score in the browser's core, settings
// and "ignore" staged unsaved, a bad `min_score` refused. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { describe, expect, it } from "vitest";

import { webLint, type CoreLintWorkspace } from "./lint";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

type Ws = CoreLintWorkspace & { dirty(): string[]; free(): void };

async function open(files: Record<string, string>): Promise<Ws> {
  const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as {
    initSync(options: { module: Uint8Array }): unknown;
    Workspace: { open(files: Record<string, string>): Ws };
  };
  mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
  return mod.Workspace.open(files);
}

const request = (name: string, url: string, extra = "") =>
  `info:\n  name: ${name}\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "${url}"\n  auth: inherit\n${extra}`;
const ASSERT = '\nruntime:\n  assertions:\n    - expression: res.status\n      operator: eq\n      value: "200"\n';

const FILES = {
  "opencollection.yml":
    'opencollection: "1.0.0"\ninfo:\n  name: sample\nrequest:\n  variables:\n    - name: baseUrl\n      value: https://api.test\n    - name: unused\n      value: 1\n',
  "a.yml": request("A", "{{baseUrl}}/a", ASSERT),
  "b.yml": request("B", "https://fixed.example.com/b"),
  "dup/one.yml": request("Same", "{{baseUrl}}/1", ASSERT),
  "dup/two.yml": request("Same", "{{baseUrl}}/2", ASSERT),
};

describe.skipIf(!built)("webLint over the real core", () => {
  it("runs the four rules and scores the collection", async () => {
    const ws = await open(FILES);
    const api = webLint(() => ({ ws }) as unknown as Session);
    const report = await api.run();
    expect(report.collection).toBe("sample");
    const by = (rule: string) => report.findings.filter((f) => f.rule === rule).map((f) => f.path).sort();
    expect(by("no-tests")).toEqual(["b.yml"]);
    expect(by("hard-coded-host")).toEqual(["b.yml"]);
    expect(by("orphan-variable")).toEqual([""]);
    expect(by("duplicate-name")).toEqual(["dup/one.yml", "dup/two.yml"]);
    expect(report.score.score).toBeGreaterThan(0);
    expect(report.score.score).toBeLessThan(100);
    expect(report.folders.map((f) => f.folder)).toEqual(["", "dup"]);
    expect(ws.dirty()).toEqual([]);
    ws.free();
  });

  it("stages settings and ignoring unsaved, and refuses a bad minimum", async () => {
    const ws = await open(FILES);
    const api = webLint(() => ({ ws }) as unknown as Session);
    const settings = await api.settings();
    expect(settings).toEqual({ rules: {}, ignore: [], min_score: null });
    await api.setSettings({ ...settings, rules: { "lint/duplicate-name": "off" }, min_score: 60 });
    expect(ws.dirty()).toEqual(["opencollection.yml"]);
    expect((await api.run()).findings.some((f) => f.rule === "duplicate-name")).toBe(false);
    await api.ignore("b.yml", "no-tests");
    const after = await api.settings();
    expect(after.ignore).toEqual(["lint/no-tests:b.yml"]);
    const report = await api.run();
    expect(report.ignored).toBe(1);
    expect(report.findings.some((f) => f.rule === "no-tests")).toBe(false);
    await expect(api.setSettings({ ...after, min_score: 101 })).rejects.toThrow();
    ws.free();
  });
});
