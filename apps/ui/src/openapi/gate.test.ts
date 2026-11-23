// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it, vi } from "vitest";

import type { SendRequirements } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { installSendGate, isSkipped, setSkipped, type GateController } from "./gate";

function req(ready: boolean): SendRequirements {
  return { operation: "GET /pets/{petId}", summary: null, deprecated: false, spec: "openapi.yaml", ready, fields: [], servers: [] };
}

function fakeApp(requirements: () => Promise<SendRequirements | null>) {
  return {
    collection: { id: "c1" },
    environment: null,
    sending: {} as Record<string, boolean>,
    sendGate: null as ((p: string) => Promise<boolean>) | null,
    backend: { openapi: { requirements: vi.fn(requirements) } },
  };
}

describe("send gate", () => {
  it("lets ready requests, requests without a spec and failures through", async () => {
    for (const r of [async () => req(true), async () => null, async () => Promise.reject(new Error("x"))]) {
      const app = fakeApp(r);
      const show = vi.fn();
      installSendGate(app as unknown as AppState, show);
      expect(await app.sendGate!("a.yml")).toBe(true);
      expect(show).not.toHaveBeenCalled();
    }
  });

  it("asks when inputs are missing; the panel decides", async () => {
    const app = fakeApp(async () => req(false));
    let controller: GateController | null = null;
    let answer: (r: "send" | "cancel") => void = () => {};
    installSendGate(app as unknown as AppState, (_p, _r, c) => {
      controller = c;
      return new Promise((resolve) => (answer = resolve));
    });
    const pending = app.sendGate!("a.yml");
    await vi.waitFor(() => expect(controller).not.toBeNull());
    expect(app.sending["a.yml"]).toBe(false);
    // Mod+Enter again while the panel is open: submits the panel, no second send.
    const submit = vi.fn();
    controller!.submit = submit;
    expect(await app.sendGate!("a.yml")).toBe(false);
    expect(submit).toHaveBeenCalledOnce();
    answer("send");
    expect(await pending).toBe(true);
    expect(app.sending["a.yml"]).toBe(true);
  });

  it("cancel does not send", async () => {
    const app = fakeApp(async () => req(false));
    installSendGate(app as unknown as AppState, async () => "cancel");
    expect(await app.sendGate!("a.yml")).toBe(false);
  });

  it("skips requests marked 'don't ask' per collection and path", async () => {
    const app = fakeApp(async () => req(false));
    const show = vi.fn();
    installSendGate(app as unknown as AppState, show);
    setSkipped("c1", "a.yml", true);
    expect(isSkipped("c1", "a.yml")).toBe(true);
    expect(isSkipped("c2", "a.yml")).toBe(false);
    expect(await app.sendGate!("a.yml")).toBe(true);
    expect(show).not.toHaveBeenCalled();
    setSkipped("c1", "a.yml", false);
    expect(isSkipped("c1", "a.yml")).toBe(false);
  });

  it("uninstalls", () => {
    const app = fakeApp(async () => null);
    const off = installSendGate(app as unknown as AppState, vi.fn());
    off();
    expect(app.sendGate).toBeNull();
  });
});
