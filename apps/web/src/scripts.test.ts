// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { TRUST_KEY, webScriptTrust, type TrustStorage } from "./scripts";
import type { Session } from "./web-backend";

function host(id: string, hasScripts = true) {
  const session = { id, ws: { hasScripts: () => hasScripts } } as unknown as Session;
  return { session: () => session };
}

function memoryStorage(): TrustStorage & { data: Map<string, string> } {
  const data = new Map<string, string>();
  return {
    data,
    getItem: (k) => data.get(k) ?? null,
    setItem: (k, v) => void data.set(k, v),
  };
}

const refusing: TrustStorage = {
  getItem() {
    throw new DOMException("blocked", "SecurityError");
  },
  setItem() {
    throw new DOMException("full", "QuotaExceededError");
  },
};

describe("web script trust", () => {
  it("starts untrusted and reports whether the collection has scripts", async () => {
    const storage = memoryStorage();
    expect(await webScriptTrust(host("a"), storage).trust()).toEqual({ has_scripts: true, trusted: false });
    expect(await webScriptTrust(host("b", false), storage).trust()).toEqual({ has_scripts: false, trusted: false });
    expect(webScriptTrust(host("a"), storage).forSend("a")).toBe(false);
  });

  it("keeps trust per collection in localStorage", async () => {
    const storage = memoryStorage();
    const a = webScriptTrust(host("a"), storage);
    await a.setTrusted(true);
    await webScriptTrust(host("b"), storage).setTrusted(true);
    expect(JSON.parse(storage.data.get(TRUST_KEY)!)).toEqual(["a", "b"]);

    // A new page (fresh module state) reads it back.
    const again = webScriptTrust(host("a"), storage);
    expect(await again.trust()).toEqual({ has_scripts: true, trusted: true });
    expect(again.forSend("b")).toBe(true);
    expect(again.forSend("c")).toBe(false);

    await again.setTrusted(false);
    expect(JSON.parse(storage.data.get(TRUST_KEY)!)).toEqual(["b"]);
    expect(webScriptTrust(host("a"), storage).forSend("a")).toBe(false);
  });

  it("ignores broken storage content", async () => {
    const storage = memoryStorage();
    storage.data.set(TRUST_KEY, "{ not json");
    const t = webScriptTrust(host("a"), storage);
    expect((await t.trust()).trusted).toBe(false);
    await t.setTrusted(true);
    expect(JSON.parse(storage.data.get(TRUST_KEY)!)).toEqual(["a"]);
    storage.data.set(TRUST_KEY, JSON.stringify({ a: true }));
    expect(webScriptTrust(host("a"), storage).forSend("a")).toBe(false);
  });

  it("falls back to memory when storage refuses or is missing", async () => {
    for (const storage of [refusing, null]) {
      const t = webScriptTrust(host("a"), storage);
      expect(t.forSend("a")).toBe(false);
      await t.setTrusted(true);
      expect(t.forSend("a")).toBe(true);
      expect(await t.trust()).toEqual({ has_scripts: true, trusted: true });
      await t.setTrusted(false);
      expect(t.forSend("a")).toBe(false);
    }
  });
});
