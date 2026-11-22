// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";
import type { AiConfig, SecretsApi } from "@requesttailor/ui";

import { CONFIG_STORAGE_KEY, KEY_COLLECTION, webAi, type AiHost } from "./ai";
import type { Session } from "./web-backend";

function memorySecrets(): Map<string, Map<string, string>> & { open(id: string): Promise<SecretsApi> } {
  const all = new Map<string, Map<string, string>>() as Map<string, Map<string, string>> & {
    open(id: string): Promise<SecretsApi>;
  };
  all.open = async (id) => {
    const m = all.get(id) ?? new Map<string, string>();
    all.set(id, m);
    return {
      has: async (e, v) => m.has(`${e}/${v}`),
      get: async (e, v) => m.get(`${e}/${v}`) ?? null,
      set: async (e, v, value) => void m.set(`${e}/${v}`, value),
      delete: async (e, v) => void m.delete(`${e}/${v}`),
    };
  };
  return all;
}

function memoryStorage(): Storage {
  const m = new Map<string, string>();
  return {
    get length() {
      return m.size;
    },
    clear: () => m.clear(),
    getItem: (k) => m.get(k) ?? null,
    key: (i) => [...m.keys()][i] ?? null,
    removeItem: (k) => void m.delete(k),
    setItem: (k, v) => void m.set(k, v),
  };
}

function setup() {
  const secrets = memorySecrets();
  const storage = memoryStorage();
  const asked: unknown[][] = [];
  const ws = {
    aiAsk: async (...args: unknown[]) => {
      asked.push(args);
      return { text: "ok" };
    },
  };
  const host: AiHost = {
    session: () => ({ id: "c1", ws }) as unknown as Session,
    openSecrets: (id) => secrets.open(id),
    sendContext: async (input) => ({
      input: { ...input, secrets: [["token", "keychain-value"]] },
      companion: null,
    }),
    storage: () => storage,
  };
  return { ai: webAi(host), secrets, storage, asked };
}

const anthropic: AiConfig = { provider: "anthropic", model: "claude-opus-5-5", base_url: null };

describe("webAi", () => {
  it("is off until configured and keeps the config in localStorage", async () => {
    const { ai, storage } = setup();
    expect(await ai.config()).toBeNull();
    await expect(ai.ask({ task: "explain-error", path: "a.yml", response: 1 }, { environment: null, vars: [] })).rejects.toMatchObject({
      kind: "not-configured",
    });
    await ai.setConfig(anthropic);
    expect(JSON.parse(storage.getItem(CONFIG_STORAGE_KEY)!)).toEqual(anthropic);
    expect(await ai.config()).toEqual(anthropic);
    await expect(ai.setConfig({ provider: "openai-compatible", model: "m", base_url: null })).rejects.toMatchObject({
      kind: "not-configured",
    });
    await expect(ai.setConfig({ ...anthropic, base_url: "ftp://x" })).rejects.toMatchObject({ kind: "transport" });
    await ai.setConfig(null);
    expect(await ai.config()).toBeNull();
  });

  it("keeps keys in the encrypted store and never returns them", async () => {
    const { ai, secrets } = setup();
    expect(await ai.hasKey("anthropic")).toBe(false);
    await ai.setKey("anthropic", " sk-1 ");
    expect(await ai.hasKey("anthropic")).toBe(true);
    expect(await ai.hasKey("openai-compatible")).toBe(false);
    expect([...secrets.get(KEY_COLLECTION)!.entries()]).toEqual([["keys/anthropic", "sk-1"]]);
    await ai.deleteKey("anthropic");
    expect(await ai.hasKey("anthropic")).toBe(false);
  });

  it("asks the core with the key, the secret values and the stored tokens for masking", async () => {
    const { ai, secrets, asked } = setup();
    await ai.setConfig(anthropic);
    await expect(ai.ask({ task: "explain-error", path: "a.yml", response: 1 }, { environment: "Local", vars: [] })).rejects.toMatchObject({
      kind: "no-key",
    });
    await ai.setKey("anthropic", "sk-1");
    const tokens = await secrets.open("c1");
    await tokens.set("oauth2", "index", JSON.stringify(["k1"]));
    await tokens.set("oauth2", "k1", JSON.stringify({ access_token: "at-1", refresh_token: "rt-1", id_token: null, obtained_at: 1 }));
    const task = { task: "explain-error" as const, path: "a.yml", response: 1 };
    expect(await ai.ask(task, { environment: "Local", vars: [["v", "1"]] })).toEqual({ text: "ok" });
    expect(asked).toEqual([
      [task, { environment: "Local", vars: [["v", "1"]], secrets: [["token", "keychain-value"]] }, anthropic, "sk-1", ["at-1", "rt-1"], null],
    ]);
  });

  it("needs no key for an OpenAI-compatible endpoint", async () => {
    const { ai, asked } = setup();
    const local: AiConfig = { provider: "openai-compatible", model: "llama3.1", base_url: "http://localhost:11434/v1" };
    await ai.setConfig(local);
    await ai.ask({ task: "create-request", description: "d", folder: "" }, { environment: null, vars: [] });
    expect(asked[0]?.[2]).toEqual(local);
    expect(asked[0]?.[3]).toBeNull();
  });
});
