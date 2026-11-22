// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";
import type { AuthPrompt, SecretsApi } from "@requesttailor/ui";

import {
  CALLBACK_PATH,
  CHANNEL,
  type AuthEnv,
  type CoreAuthCallbacks,
  type StoredToken,
  TOKEN_ENVIRONMENT,
  WebTokens,
  keyLabel,
  webAuth,
} from "./auth";
import type { WebHost } from "./web-backend";

function memorySecrets() {
  const data = new Map<string, string>();
  const k = (env: string, name: string) => `${env}\u0000${name}`;
  const api: SecretsApi = {
    has: async (env, name) => data.has(k(env, name)),
    get: async (env, name) => data.get(k(env, name)) ?? null,
    set: async (env, name, value) => void data.set(k(env, name), value),
    delete: async (env, name) => void data.delete(k(env, name)),
  };
  return { api, data };
}

const token = (access: string, at: number): StoredToken => ({
  access_token: access,
  refresh_token: "r",
  id_token: null,
  token_type: "Bearer",
  scope: null,
  expires_at: at + 1000,
  obtained_at: at,
});

const KEY = "oauth2|1|client-credentials|https://idp.example:8443/token|cli|read write||";

/** A BroadcastChannel stand-in whose messages the test posts. */
function fakeEnv(origin = "http://127.0.0.1:5185") {
  const channels: { name: string; ch: { onmessage: ((e: MessageEvent) => void) | null; close(): void } }[] = [];
  const opened: string[] = [];
  const env: AuthEnv = {
    origin,
    open: (url) => {
      opened.push(url);
      return { close() {} };
    },
    channel: (name) => {
      const ch = { onmessage: null as ((e: MessageEvent) => void) | null, close() {} };
      channels.push({ name, ch });
      return ch;
    },
  };
  const post = (url: string) => {
    for (const { ch } of channels) ch.onmessage?.({ data: { url } } as MessageEvent);
  };
  return { env, opened, channels, post };
}

function host(secrets: SecretsApi): WebHost {
  return {
    deps: { openSecrets: async () => secrets } as never,
    session: () => ({ id: "c1" }) as never,
    openHandle: async () => {
      throw new Error("unused");
    },
    companionCredentials: async () => null,
  };
}

describe("web OAuth 2.0 tokens", () => {
  it("labels keys like the core", () => {
    expect(keyLabel(KEY)).toBe("idp.example · cli · read write");
    expect(keyLabel("oauth2|1|password|https://idp/t|c|||user=alice")).toBe("idp · c · alice");
    expect(keyLabel("oauth2|1|x|not a url%7Cx|c|||")).toBe("not a url|x · c");
    expect(keyLabel("other")).toBe("other");
  });

  it("keeps tokens encrypted-store side, lists them without values", async () => {
    const { api, data } = memorySecrets();
    const tokens = new WebTokens(api);
    await tokens.put(KEY, token("secret-a", 2));
    await tokens.put("oauth2|1|b|https://h/t|c|||", token("secret-b", 1));
    await tokens.put(KEY, token("secret-a2", 3));
    expect([...data.keys()].every((k) => k.startsWith(`${TOKEN_ENVIRONMENT}\u0000`))).toBe(true);
    const list = await tokens.list();
    expect(list.map((t) => t.key)).toEqual(["oauth2|1|b|https://h/t|c|||", KEY]);
    expect(list[1]).toMatchObject({ label: "idp.example · cli · read write", refreshable: true });
    expect(JSON.stringify(list)).not.toContain("secret");
    expect((await tokens.get(KEY))?.access_token).toBe("secret-a2");
    await tokens.clear(KEY);
    expect(await tokens.list()).toHaveLength(1);
    await tokens.clear(null);
    expect(data.size).toBe(0);
  });
});

describe("web OAuth 2.0 prompts", () => {
  it("returns the redirect from the callback page and reports the steps", async () => {
    const { env, opened, post, channels } = fakeEnv();
    const { api } = memorySecrets();
    const auth = webAuth(host(api), env);
    const seen: AuthPrompt[] = [];
    auth.onPrompt((p) => seen.push(p));
    const cb = (await auth.forSend("c1", null)) as CoreAuthCallbacks;
    const redirect = `http://127.0.0.1:5185${CALLBACK_PATH}`;
    const done = cb.authorize("https://idp/authorize?state=s", redirect);
    expect(opened).toEqual(["https://idp/authorize?state=s"]);
    expect(channels[0]!.name).toBe(CHANNEL);
    post("http://evil.example/oauth-callback.html?code=x"); // other origin: ignored
    post(`${redirect}?code=c1&state=s`);
    await expect(done).resolves.toBe(`${redirect}?code=c1&state=s`);
    expect(seen).toEqual([{ kind: "browser", url: "https://idp/authorize?state=s" }, { kind: "done" }]);

    cb.deviceCode({ user_code: "AB-CD", verification_uri: "https://idp/d", verification_uri_complete: null, expires_in: 60 });
    cb.deviceCodeDone();
    expect(seen.slice(2)).toEqual([
      { kind: "device-code", user_code: "AB-CD", verification_uri: "https://idp/d", verification_uri_complete: null, expires_in: 60 },
      { kind: "done" },
    ]);
  });

  it("refuses a callback URL the app cannot receive", async () => {
    const { env, opened } = fakeEnv();
    const cb = (await webAuth(host(memorySecrets().api), env).forSend("c1", null)) as CoreAuthCallbacks;
    await expect(cb.authorize("https://idp/a", "http://127.0.0.1:8765/callback")).rejects.toThrow(
      /must be http:\/\/127\.0\.0\.1:5185\/oauth-callback\.html/,
    );
    expect(opened).toEqual([]);
  });

  it("cancel stops the waiting browser step and the device code polling", async () => {
    const { env } = fakeEnv();
    const auth = webAuth(host(memorySecrets().api), env);
    const cb = (await auth.forSend("c1", null)) as CoreAuthCallbacks;
    const waiting = cb.authorize("https://idp/a", `http://127.0.0.1:5185${CALLBACK_PATH}`);
    expect(cb.cancelled()).toBe(false);
    await auth.cancelPrompt();
    await expect(waiting).rejects.toThrow("cancelled");
    expect(cb.cancelled()).toBe(true);
    // The next flow starts uncancelled.
    const next = (await auth.forSend("c1", null)) as CoreAuthCallbacks;
    expect(next.cancelled()).toBe(false);
  });

  it("lists and clears the open collection's tokens", async () => {
    const { api } = memorySecrets();
    const auth = webAuth(host(api), fakeEnv().env);
    const cb = (await auth.forSend("c1", null)) as CoreAuthCallbacks;
    await cb.put(KEY, token("v", 1));
    expect((await auth.tokens()).map((t) => t.key)).toEqual([KEY]);
    await auth.clearTokens(null);
    expect(await auth.tokens()).toEqual([]);
  });
});
