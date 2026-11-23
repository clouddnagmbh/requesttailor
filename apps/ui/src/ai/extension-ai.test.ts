// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { Backend } from "../commands/backend";
import { mockAi } from "../commands/mock-ai";
import type { AppState } from "../state/app.svelte";
import { createExtensionAi } from "./extension-ai";
import { segments } from "./preview";

async function setup() {
  const ai = mockAi();
  await ai.setSettings({
    profiles: [
      { id: "quick", label: "Quick", provider: "openai-compatible", model: "small", base_url: "http://localhost:1/v1" },
      { id: "big", label: "Big", provider: "anthropic", model: "opus", base_url: null },
    ],
    roles: { fast: "quick", smart: "big" },
    preview: "first-use",
  });
  await ai.setKey("anthropic", "k");
  const shown: string[] = [];
  let answer = true;
  const ext = createExtensionAi({ ai } as unknown as Backend, { environment: null } as unknown as AppState, async (p) => {
    shown.push(p.profile);
    return answer;
  });
  const req = { purpose: "Explain", profile: "smart", system: "s", messages: [{ role: "user" as const, content: "token sk-secret-1" }] };
  return { ai, ext, shown, req, say: (v: boolean) => (answer = v) };
}

describe("ExtensionContext.ai", () => {
  it("shows the preview the first time of a profile, remembers the confirmation, then sends", async () => {
    const { ai, ext, shown, req } = await setup();
    ai.secrets = ["sk-secret-1"];
    expect((await ext.status()).profiles.map((p) => p.id)).toEqual(["quick", "big"]);
    const deltas: unknown[] = [];
    const reply = await ext.complete(req, { onDelta: (d) => deltas.push(d) });
    expect(reply.text).toContain("Explain");
    expect(shown).toEqual(["big"]);
    expect(deltas.length).toBeGreaterThan(0);
    // What was sent is what was previewed: masked.
    expect(JSON.stringify(ai.sent[0])).not.toContain("sk-secret-1");
    expect(JSON.stringify(ai.sent[0])).toContain("***");
    await ext.complete(req);
    expect(shown).toEqual(["big"]);
    // Another profile is a first use of its own; the role picks it.
    await ext.complete({ ...req, profile: "fast" });
    expect(shown).toEqual(["big", "quick"]);
  });

  it("cancelling the preview sends nothing and asks nothing more", async () => {
    const { ai, ext, shown, req, say } = await setup();
    say(false);
    await expect(ext.complete(req)).rejects.toMatchObject({ kind: "cancelled" });
    expect(shown).toEqual(["big"]);
    expect(ai.sent).toEqual([]);
    expect((await ai.settings())?.previewed ?? []).toEqual([]);
  });

  it("an opted-out collection never builds a call", async () => {
    const { ai, ext, shown, req } = await setup();
    await ai.setDeviceOptOut(true);
    await expect(ext.complete(req)).rejects.toMatchObject({ kind: "opted-out" });
    expect((await ext.status()).allowed).toBe(false);
    await ai.setDeviceOptOut(false);
    ai.fileOff = true;
    await expect(ext.complete(req)).rejects.toMatchObject({ kind: "opted-out" });
    expect(shown).toEqual([]);
    expect(ai.sent).toEqual([]);
  });

  it("'always' shows the preview every time; the abort signal cancels", async () => {
    const { ai, ext, shown, req } = await setup();
    const s = (await ai.settings())!;
    await ai.setSettings({ ...s, preview: "always" });
    await ext.complete(req);
    await ext.complete(req);
    expect(shown).toEqual(["big", "big"]);
    const ctl = new AbortController();
    const run = ext.complete(req, { signal: ctl.signal });
    ctl.abort();
    await expect(run).rejects.toMatchObject({ kind: "cancelled" });
  });
});

describe("preview text", () => {
  it("marks the places secrets were replaced", () => {
    expect(segments("a *** b")).toEqual([
      { text: "a ", masked: false },
      { text: "***", masked: true },
      { text: " b", masked: false },
    ]);
    expect(segments("plain")).toEqual([{ text: "plain", masked: false }]);
  });
});
