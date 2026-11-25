// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The web app's security API (wave 19, strand R) over the real core (rt-wasm, built with
// `node scripts/build-wasm.mjs`): the rule list, the device settings in localStorage and how
// they reach the checks. Skipped when the core is not built.

import * as nodeFs from "node:fs";
import { join } from "node:path";

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { readSettings, securityIgnored, SETTINGS_KEY, webSecurity, type CoreSecurityModule } from "./security";
import type { CoreModule } from "./wasm";
import type { Session } from "./web-backend";

const WASM = join(__dirname, "../public/wasm");
const built = nodeFs.existsSync(join(WASM, "rt_wasm_bg.wasm"));

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

describe("the settings of this device", () => {
  it("default to everything on and survive a broken storage", () => {
    expect(readSettings()).toEqual({ enabled: true, disabled: [] });
    localStorage.setItem(SETTINGS_KEY, JSON.stringify({ enabled: false, disabled: ["a", 1, "b"] }));
    expect(readSettings()).toEqual({ enabled: false, disabled: ["a", "b"] });
    localStorage.setItem(SETTINGS_KEY, "{broken");
    expect(readSettings()).toEqual({ enabled: true, disabled: [] });
    vi.stubGlobal("localStorage", undefined);
    expect(readSettings()).toEqual({ enabled: true, disabled: [] });
  });

  it("need no core while everything is on", async () => {
    const core = vi.fn();
    expect(await securityIgnored(core as unknown as () => Promise<CoreModule>)).toEqual([]);
    expect(core).not.toHaveBeenCalled();
  });
});

describe.skipIf(!built)("webSecurity over the real core", () => {
  type Mod = CoreSecurityModule & { Workspace: { open(files: Record<string, string>): unknown } };

  async function load(): Promise<Mod> {
    const mod = (await import(/* @vite-ignore */ join(WASM, "rt_wasm.js"))) as Mod & {
      initSync(options: { module: Uint8Array }): unknown;
    };
    mod.initSync({ module: nodeFs.readFileSync(join(WASM, "rt_wasm_bg.wasm")) });
    return mod;
  }

  async function api() {
    const mod = await load();
    const ws = mod.Workspace.open({ "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: sample\n' });
    const host = { session: () => ({ ws }) as unknown as Session, core: async () => mod as unknown as CoreModule };
    return { api: webSecurity(host), mod };
  }

  it("lists the rules with their OWASP category, TLS ones native only", async () => {
    const { api: security } = await api();
    const rules = await security.rules();
    expect(rules.length).toBeGreaterThanOrEqual(17);
    expect(rules.find((r) => r.id === "sec/tls-cert-expired")).toMatchObject({ category: "API8:2023", native_only: true });
    expect(rules.find((r) => r.id === "sec/jwt-weak-hmac")?.category).toBe("API2:2023");
  });

  it("keeps the settings in storage and turns them into the ids the checks skip", async () => {
    const { api: security, mod } = await api();
    expect(await security.settings()).toEqual({ enabled: true, disabled: [] });
    await security.setSettings({ enabled: true, disabled: ["sec/no-rate-limit"] });
    expect(JSON.parse(localStorage.getItem(SETTINGS_KEY)!)).toEqual({ enabled: true, disabled: ["sec/no-rate-limit"] });
    expect(await securityIgnored(async () => mod as unknown as CoreModule)).toEqual(["sec/no-rate-limit"]);
    await security.setSettings({ enabled: false, disabled: [] });
    const all = await securityIgnored(async () => mod as unknown as CoreModule);
    expect(all).toContain("sec/tls-weak-key");
    expect(all).not.toContain("chk/hsts-missing");
  });
});
