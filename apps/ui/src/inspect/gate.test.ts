// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { RequestSpec } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { tokenGate } from "./gate";
import { WARN_KEY, setWarningEnabled, warningEnabled } from "./settings";

const PATH = "users/list-users.yml";

function jwt(claims: Record<string, unknown>, alg = "RS256"): string {
  const part = (v: unknown) => btoa(JSON.stringify(v)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
  return `${part({ alg })}.${part(claims)}.c2ln`;
}

const now = () => Math.floor(Date.now() / 1000);

async function setup(token: string | null, extra: Partial<RequestSpec> = {}) {
  const backend = new MockBackend();
  await backend.openCollection();
  const spec = await backend.getRequest(PATH);
  await backend.updateRequest(PATH, {
    ...spec,
    headers: token ? [...spec.headers, { name: "Authorization", value: `Bearer ${token}`, enabled: true }] : spec.headers,
    ...extra,
  });
  return { backend, app: new AppState(backend) };
}

/** Waits until the gate has put its dialog up. */
const dialogOf = async (app: AppState) => {
  await vi.waitFor(() => expect(app.dialog).not.toBeNull());
  return app.dialog!.dialog;
};

function fakeStorage(): Storage {
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

beforeEach(() => {
  vi.stubGlobal("localStorage", fakeStorage());
});
afterEach(() => {
  vi.unstubAllGlobals();
});

describe("the token warning before a send", () => {
  it("says nothing for a good token, no token, or a broken backend", async () => {
    const good = await setup(jwt({ sub: "a", exp: now() + 3600 }));
    expect(await tokenGate(good.app, PATH)).toBe(true);
    expect(good.app.dialog).toBeNull();
    const none = await setup(null);
    expect(await tokenGate(none.app, PATH)).toBe(true);
    const broken = await setup(jwt({ exp: 1 }));
    vi.spyOn(broken.backend.inspect, "tokens").mockRejectedValue(new Error("down"));
    expect(await tokenGate(broken.app, PATH)).toBe(true);
    expect(broken.app.dialog).toBeNull();
  });

  it("asks about an expired token, and Send anyway lets it go (Enter)", async () => {
    const { app } = await setup(jwt({ sub: "RT-CANARY", exp: now() - 720 }));
    const asked = tokenGate(app, PATH);
    const dialog = await dialogOf(app);
    expect(dialog.title).toBe("Send with this token?");
    expect(dialog.props).toMatchObject({ text: "Token expired 12 min ago.", where: "Authorization header", oauth: false });
    // The value of the token is nowhere in what the dialog is given.
    expect(JSON.stringify(dialog.props)).not.toContain("RT-CANARY");
    app.closeDialog("send");
    expect(await asked).toBe(true);
  });

  it("cancels with Escape (the dialog closes with nothing)", async () => {
    const { app } = await setup(jwt({ exp: now() - 5 }));
    const asked = tokenGate(app, PATH);
    await dialogOf(app);
    app.closeDialog(undefined);
    expect(await asked).toBe(false);
  });

  it("also asks about a token that is not valid yet or runs out within 30 seconds", async () => {
    for (const claims of [{ nbf: now() + 600 }, { exp: now() + 10 }]) {
      const { app } = await setup(jwt(claims));
      const asked = tokenGate(app, PATH);
      await dialogOf(app);
      app.closeDialog("send");
      expect(await asked).toBe(true);
    }
  });

  it("can be switched off on this device", async () => {
    const { app } = await setup(jwt({ exp: now() - 5 }));
    expect(warningEnabled()).toBe(true);
    setWarningEnabled(false);
    expect(localStorage.getItem(WARN_KEY)).toBe("off");
    expect(await tokenGate(app, PATH)).toBe(true);
    expect(app.dialog).toBeNull();
    setWarningEnabled(true);
    expect(warningEnabled()).toBe(true);
  });

  it("offers a new token when the request has an OAuth auth block, and fetches it on request", async () => {
    const oauth = { type: "oauth2" } as unknown as RequestSpec["auth"];
    const { app, backend } = await setup(jwt({ exp: now() - 5 }), { auth: oauth });
    const fetched = vi.spyOn(backend.auth, "fetchToken").mockResolvedValue({ key: "k", label: "l", obtained_at: 0, expires_at: null, refreshable: false });
    const asked = tokenGate(app, PATH);
    const dialog = await dialogOf(app);
    expect(dialog.props).toMatchObject({ oauth: true });
    app.closeDialog("new-token");
    expect(await asked).toBe(true);
    expect(fetched).toHaveBeenCalledOnce();
  });

  it("works without storage: the warning stays on", () => {
    vi.stubGlobal("localStorage", undefined);
    expect(warningEnabled()).toBe(true);
    expect(() => setWarningEnabled(false)).not.toThrow();
  });
});
