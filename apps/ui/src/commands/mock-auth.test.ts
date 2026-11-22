// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import { mockAuth } from "./mock-auth";
import type { AuthPrompt } from "./types";

const input = { environment: "Local", vars: [] as [string, string][] };

describe("mockAuth", () => {
  it("runs a browser flow, stores the token and lists it without a value", async () => {
    const auth = mockAuth(10);
    const prompts: AuthPrompt[] = [];
    auth.onPrompt((p) => prompts.push(p));
    const info = await auth.fetchToken("users/me.yml", input);
    expect(info).toMatchObject({ key: "mock|users/me.yml", refreshable: true });
    expect(prompts.map((p) => p.kind)).toEqual(["browser", "done"]);
    expect(await auth.tokens()).toEqual([info]);
    expect(JSON.stringify(await auth.tokens())).not.toContain("access_token");
  });

  it("shows a device code for device requests", async () => {
    const auth = mockAuth(10);
    const prompts: AuthPrompt[] = [];
    auth.onPrompt((p) => prompts.push(p));
    await auth.fetchToken("device/me.yml", input);
    expect(prompts[0]).toMatchObject({ kind: "device-code", user_code: "WDJB-MJHT", expires_in: 600 });
  });

  it("cancels a waiting flow", async () => {
    const auth = mockAuth(1000);
    const prompts: AuthPrompt[] = [];
    const off = auth.onPrompt((p) => prompts.push(p));
    const pending = auth.fetchToken("a.yml", input);
    await auth.cancelPrompt();
    await expect(pending).rejects.toEqual(new BackendError("auth", "cancelled"));
    expect(prompts.map((p) => p.kind)).toEqual(["browser", "done"]);
    expect(await auth.tokens()).toEqual([]);
    off();
  });

  it("clears one or all tokens", async () => {
    const auth = mockAuth(1);
    await auth.fetchToken("a.yml", input);
    await auth.fetchToken("b.yml", input);
    await auth.clearTokens("mock|a.yml");
    expect((await auth.tokens()).map((t) => t.key)).toEqual(["mock|b.yml"]);
    await auth.clearTokens(null);
    expect(await auth.tokens()).toEqual([]);
  });
});
