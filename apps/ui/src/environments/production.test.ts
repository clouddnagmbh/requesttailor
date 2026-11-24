// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: how the app shows and guards a production environment. One rule (the
// policy flag, else the name, as `rt_app::agents::is_production`), a frame and a label, and a
// confirmation only for writing requests.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AgentPolicy } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { isProductionName } from "./ops";
import {
  confirmEnabled,
  confirmQuestion,
  isProduction,
  sendLabel,
  setConfirmEnabled,
  shouldConfirm,
} from "./production";
import { getProduction } from "./production.svelte";

function memoryStorage() {
  const data = new Map<string, string>();
  return {
    getItem: (k: string) => data.get(k) ?? null,
    setItem: (k: string, v: string) => void data.set(k, v),
    removeItem: (k: string) => void data.delete(k),
  };
}

const policy = (...entries: [string, boolean][]): AgentPolicy => ({
  access: "read",
  environments: entries.map(([environment, production]) => ({ environment, access: "write", production })),
});

describe("is_production in the UI", () => {
  it("is the flag when the policy lists the environment, the name otherwise", () => {
    expect(isProduction(policy(["Customer-A", true]), "Customer-A")).toBe(true);
    expect(isProduction(policy(["Customer-A", true]), "Customer-B")).toBe(false);
    expect(isProduction(policy(["prod-sandbox", false]), "prod-sandbox")).toBe(false);
    expect(isProduction(null, "PRD")).toBe(true);
    expect(isProduction(null, null)).toBe(false);
    for (const n of ["PRD", "prod", "eu-prod-2", "Live", "Local", "delivery", "alive"]) {
      expect(isProduction({ access: "read", environments: [] }, n), n).toBe(isProductionName(n));
    }
  });
});

describe("label and question", () => {
  it("names production on the send button", () => {
    expect(sendLabel(false, false)).toBe("Send");
    expect(sendLabel(true, false)).toBe("Send to PRODUCTION");
    expect(sendLabel(true, true)).toBe("Sending…");
  });

  it("asks with method, url and environment", () => {
    expect(confirmQuestion({ method: "POST", url: "{{base}}/orders", environment: "PRD" })).toBe("Send POST to PRD?");
  });

  it("asks only for writing requests in production, and only while it is on", () => {
    expect(shouldConfirm({ production: true, effect: "write", enabled: true })).toBe(true);
    expect(shouldConfirm({ production: true, effect: "read", enabled: true })).toBe(false);
    expect(shouldConfirm({ production: false, effect: "write", enabled: true })).toBe(false);
    expect(shouldConfirm({ production: true, effect: "write", enabled: false })).toBe(false);
  });
});

describe("the confirmation on this device", () => {
  beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
  afterEach(() => vi.unstubAllGlobals());

  it("is on until it is switched off, and stays off", () => {
    expect(confirmEnabled()).toBe(true);
    setConfirmEnabled(false);
    expect(confirmEnabled()).toBe(false);
    setConfirmEnabled(true);
    expect(confirmEnabled()).toBe(true);
  });
});

describe("ProductionState", () => {
  beforeEach(() => vi.stubGlobal("localStorage", memoryStorage()));
  afterEach(() => vi.unstubAllGlobals());

  async function opened(environment: string | null) {
    const app = new AppState(new MockBackend());
    await app.open();
    app.setEnvironment(environment);
    const production = getProduction(app);
    await production.refresh();
    return { app, production };
  }

  it("is active for a production environment only", async () => {
    expect((await opened("PRD")).production.active).toBe(true);
    expect((await opened("Local")).production.active).toBe(false);
    expect((await opened(null)).production.active).toBe(false);
  });

  it("lets reading requests and other environments through without asking", async () => {
    const prd = await opened("PRD");
    expect(await prd.production.gate("users/list-users.yml")).toBe(true);
    expect(prd.production.pending).toBeNull();
    const local = await opened("Local");
    expect(await local.production.gate("users/create-user.yml")).toBe(true);
    expect(local.production.pending).toBeNull();
  });

  it("holds a writing request until Enter or Escape answers", async () => {
    const { production } = await opened("PRD");
    const yes = production.gate("users/create-user.yml");
    await Promise.resolve();
    await new Promise((r) => setTimeout(r));
    expect(production.pending).toMatchObject({ environment: "PRD" });
    expect(production.pending?.method).toBe("POST");
    production.answer(true);
    expect(await yes).toBe(true);
    expect(production.pending).toBeNull();

    const no = production.gate("users/create-user.yml");
    await new Promise((r) => setTimeout(r));
    production.answer(false);
    expect(await no).toBe(false);
  });

  it("does not ask once the device switched the question off", async () => {
    const { production } = await opened("PRD");
    setConfirmEnabled(false);
    expect(await production.gate("users/create-user.yml")).toBe(true);
    expect(production.pending).toBeNull();
  });

  it("asks once per run, whatever the requests are", async () => {
    const { production } = await opened("PRD");
    const run = production.gateRun("the collection");
    await new Promise((r) => setTimeout(r));
    expect(production.pending?.kind).toBe("run");
    production.answer(true);
    expect(await run).toBe(true);
    const local = await opened("Local");
    expect(await local.production.gateRun("the collection")).toBe(true);
  });
});
