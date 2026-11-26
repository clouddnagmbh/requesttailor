// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Tailor Kit Authors

import { afterEach, describe, expect, it, vi } from "vitest";

import { HintStore } from "./hint-store";
import { Theme } from "./theme.svelte";

// What users stored must stay readable: the keys keep RequestTailor's prefix.
describe("RequestTailor storage keys", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("reads and writes rt.shell.* for the theme and the hint store", () => {
    const data = new Map<string, string>([["rt.shell.theme", '"dark"']]);
    vi.stubGlobal("localStorage", { getItem: (k: string) => data.get(k) ?? null, setItem: (k: string, v: string) => void data.set(k, v), removeItem: (k: string) => void data.delete(k) });
    const theme = new Theme(undefined);
    expect(theme.preference).toBe("dark");
    theme.set("light");
    expect(data.get("rt.shell.theme")).toBe('"light"');
    new HintStore().dismiss("a");
    expect(data.get("rt.shell.hints.dismissed")).toBe('["a"]');
  });
});
