// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";
import type { AiConfig, AiSettings, SecretsApi } from "@requesttailor/ui";

import { CONFIG_STORAGE_KEY, KEY_COLLECTION, OFF_STORAGE_KEY, parseSettings, webAi, type AiHost } from "./ai";
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

/** The core's AI functions are in Rust (crates/rt-wasm/src/ai_js.rs, tested there and in the
 *  shells e2e); this stub only checks the plumbing. */
const core = {
  aiSettingsNormalize: (next: AiSettings, old: AiSettings | null) => {
    if (next.profiles.some((p) => p.model.trim() === "")) {
      throw Object.assign(new Error("no AI provider is set up"), { kind: "not-configured" });
    }
    return { ...next, previewed: (old?.previewed ?? []).filter((id) => next.profiles.some((p) => p.id === id)) };
  },
  aiSettingsConfirm: (s: AiSettings, id: string) => ({ ...s, previewed: [...(s.previewed ?? []), id] }),
  aiCancel: (id: string) => void cancelled.push(id),
};
const cancelled: string[] = [];

function setup(companion: { url: string; token: string } | null = null) {
  const secrets = memorySecrets();
  const storage = memoryStorage();
  const calls: { method: string; args: unknown[] }[] = [];
  const ws = {
    aiPolicy: (off: boolean) => ({ file_off: false, device_off: off }),
    aiPreview: (...args: unknown[]) => (calls.push({ method: "preview", args }), { profile: "x", required: true }),
    aiAsk: async (...args: unknown[]) => (calls.push({ method: "ask", args }), { text: "ok" }),
    aiComplete: async (...args: unknown[]) => (calls.push({ method: "complete", args }), { text: "done" }),
  };
  const host: AiHost = {
    session: () => ({ id: "c1", ws }) as unknown as Session,
    openSecrets: (id) => secrets.open(id),
    sendContext: async (input) => ({
      input: { ...input, secrets: [["token", "keychain-value"]] },
      companion,
    }),
    core: async () => core,
    storage: () => storage,
  };
  return { ai: webAi(host), secrets, storage, calls };
}

const anthropic: AiConfig = { provider: "anthropic", model: "claude-opus-5-5", base_url: null };
const local: AiConfig = { provider: "openai-compatible", model: "llama3.1", base_url: "http://localhost:11434/v1" };
const input = { environment: "Local", vars: [] as [string, string][] };
const explain = { task: "explain-error" as const, path: "a.yml", response: 1 };

describe("webAi", () => {
  it("is off until configured and reads a wave-8 config as the profile default", async () => {
    const { ai, storage } = setup();
    expect(await ai.config()).toBeNull();
    expect(await ai.settings()).toBeNull();
    await expect(ai.ask(explain, input)).rejects.toMatchObject({ kind: "not-configured" });
    storage.setItem(CONFIG_STORAGE_KEY, JSON.stringify(anthropic));
    const s = await ai.settings();
    expect(s?.profiles).toEqual([{ id: "default", label: "Anthropic", ...anthropic }]);
    expect(await ai.config()).toEqual(anthropic);
    await ai.setConfig(null);
    expect(await ai.settings()).toBeNull();
    expect(parseSettings("not json")).toBeNull();
  });

  it("checks the old setConfig the way the desktop app does", async () => {
    const { ai } = setup();
    await expect(ai.setConfig({ provider: "openai-compatible", model: "m", base_url: null })).rejects.toMatchObject({ kind: "not-configured" });
    await expect(ai.setConfig({ ...anthropic, base_url: "ftp://x" })).rejects.toMatchObject({ kind: "transport" });
    await ai.setConfig(anthropic);
    expect((await ai.settings())?.profiles[0]).toMatchObject({ id: "default", provider: "anthropic" });
  });

  it("keeps keys per provider and per profile in the encrypted store and never returns them", async () => {
    const { ai, secrets } = setup();
    await ai.setKey("anthropic", " sk-1 ");
    await ai.setProfileKey("big", "sk-2");
    expect(await ai.hasKey("anthropic")).toBe(true);
    expect(await ai.hasProfileKey("big")).toBe(true);
    expect(await ai.hasProfileKey("small")).toBe(false);
    expect([...secrets.get(KEY_COLLECTION)!.entries()]).toEqual([
      ["keys/anthropic", "sk-1"],
      ["keys/profile:big", "sk-2"],
    ]);
    await ai.deleteKey("anthropic");
    await ai.deleteProfileKey("big");
    expect(await ai.hasKey("anthropic")).toBe(false);
  });

  it("a profile's own key wins over its provider's; roles pick the profile", async () => {
    const { ai, calls } = setup();
    await ai.setSettings({
      profiles: [
        { id: "quick", label: "Quick", provider: "anthropic", model: "haiku", base_url: null },
        { id: "big", label: "Big", provider: "anthropic", model: "opus", base_url: null },
      ],
      default_profile: "big",
      roles: { fast: "quick", smart: "big" },
      preview: "first-use",
    });
    await ai.setKey("anthropic", "provider-key");
    await ai.setProfileKey("quick", "own-key");
    await ai.ask({ task: "suggest-names", target: "request", path: "a.yml" }, input);
    await ai.ask(explain, input);
    expect(calls.map((c) => c.args[3])).toEqual(["own-key", "provider-key"]);
    expect((calls[0]!.args[2] as AiSettings).profiles).toHaveLength(2);
  });

  it("asks the core with the key, the secret values and the stored tokens for masking", async () => {
    const { ai, secrets, calls } = setup();
    await ai.setConfig(anthropic);
    await expect(ai.ask(explain, input)).rejects.toMatchObject({ kind: "no-key" });
    await ai.setKey("anthropic", "sk-1");
    const tokens = await secrets.open("c1");
    await tokens.set("oauth2", "index", JSON.stringify(["k1"]));
    await tokens.set("oauth2", "k1", JSON.stringify({ access_token: "at-1", refresh_token: "rt-1", id_token: null, obtained_at: 1 }));
    expect(await ai.ask(explain, { environment: "Local", vars: [["v", "1"]] })).toEqual({ text: "ok" });
    const args = calls[0]!.args;
    expect(args[0]).toEqual(explain);
    expect(args[1]).toEqual({ environment: "Local", vars: [["v", "1"]], secrets: [["token", "keychain-value"]] });
    expect(args[3]).toBe("sk-1");
    expect(args[4]).toEqual(["at-1", "rt-1"]);
    expect(args[6]).toBeNull();
  });

  it("needs no key for an OpenAI-compatible endpoint and calls it directly", async () => {
    const { ai, calls } = setup({ url: "http://127.0.0.1:1", token: "t" });
    await ai.setConfig(local);
    await ai.ask({ task: "create-request", description: "d", folder: "" }, { environment: null, vars: [] });
    expect(calls[0]?.args[3]).toBeNull();
    expect(calls[0]?.args[6]).toBeNull();
  });

  it("a provider that does not allow web pages goes through the companion, or is refused without sending", async () => {
    const without = setup();
    await without.ai.setSettings({ profiles: [{ id: "m", label: "M", provider: "mistral", model: "m", base_url: null }], roles: {}, preview: "first-use" });
    await without.ai.setKey("mistral", "k");
    await expect(without.ai.ask(explain, input)).rejects.toMatchObject({ kind: "transport", message: expect.stringContaining("companion") });
    expect(without.calls).toEqual([]);
    // The preview needs no companion: it sends nothing.
    await without.ai.preview({ kind: "task", task: explain }, input);

    const withIt = setup({ url: "http://127.0.0.1:48334", token: "t" });
    await withIt.ai.setSettings({ profiles: [{ id: "m", label: "M", provider: "mistral", model: "m", base_url: null }], roles: {}, preview: "first-use" });
    await withIt.ai.setKey("mistral", "k");
    await withIt.ai.ask(explain, input);
    expect(withIt.calls[0]?.args[6]).toEqual({ url: "http://127.0.0.1:48334", token: "t" });
  });

  it("opt-out per device, first-use confirmation and cancel go through", async () => {
    const { ai, storage, calls } = setup();
    await ai.setConfig(anthropic);
    expect(await ai.policy()).toEqual({ file_off: false, device_off: false });
    await ai.setDeviceOptOut(true);
    expect(JSON.parse(storage.getItem(OFF_STORAGE_KEY)!)).toEqual(["c1"]);
    expect((await ai.policy()).device_off).toBe(true);
    await ai.setDeviceOptOut(false);
    expect(JSON.parse(storage.getItem(OFF_STORAGE_KEY)!)).toEqual([]);
    await ai.confirmPreview("default");
    expect((await ai.settings())?.previewed).toEqual(["default"]);
    await ai.setKey("anthropic", "k");
    const req = { id: "r1", purpose: "p", system: "s", messages: [], scope: {} };
    const events: unknown[] = [];
    await ai.complete(req, input, (e) => events.push(e));
    expect(calls.at(-1)?.method).toBe("complete");
    await ai.cancel("r1");
    expect(cancelled).toEqual(["r1"]);
  });
});
