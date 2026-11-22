// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";
import type { AuthPrompt } from "@requesttailor/ui";

import { PROMPT_EVENT, tauriAuth, type Listen } from "./auth";
import type { Call } from "./backend";

function fakeListen() {
  const handlers = new Map<string, (event: { payload: AuthPrompt }) => void>();
  let release: () => void = () => {};
  const ready = new Promise<void>((r) => (release = r));
  const listen: Listen = async (event, handler) => {
    await ready;
    handlers.set(event, handler);
    return () => handlers.delete(event);
  };
  return { listen, handlers, release };
}

describe("desktop auth", () => {
  it("delivers the auth-prompt event and unsubscribes", async () => {
    const call = (async () => null) as Call;
    const { listen, handlers, release } = fakeListen();
    const auth = tauriAuth(call, listen);
    const seen: AuthPrompt[] = [];
    const stop = auth.onPrompt((p) => seen.push(p));
    release();
    await new Promise((r) => setTimeout(r, 0));
    handlers.get(PROMPT_EVENT)!({ payload: { kind: "browser", url: "https://idp/auth" } });
    expect(seen).toEqual([{ kind: "browser", url: "https://idp/auth" }]);
    stop();
    expect(handlers.size).toBe(0);
  });

  it("unsubscribing before listen resolved still unsubscribes", async () => {
    const { listen, handlers, release } = fakeListen();
    const auth = tauriAuth((async () => null) as Call, listen);
    const stop = auth.onPrompt(() => {});
    stop();
    release();
    await new Promise((r) => setTimeout(r, 0));
    expect(handlers.size).toBe(0);
  });

  it("never sends secret values with fetchToken", async () => {
    const calls: [string, unknown][] = [];
    const call = (async (cmd: string, args?: unknown) => {
      calls.push([cmd, args]);
      return null;
    }) as Call;
    const auth = tauriAuth(call, fakeListen().listen);
    const input = { environment: "Local", vars: [], secrets: [["s", "leak"]] };
    await auth.fetchToken("a.yml", input as never);
    expect(calls).toEqual([
      ["auth_fetch_token", { path: "a.yml", input: { environment: "Local", vars: [] } }],
    ]);
  });
});
