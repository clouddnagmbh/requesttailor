// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { matchesKey } from "./key";

const ev = (key: string, m: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...m,
});

describe("matchesKey", () => {
  it("Mod is Cmd on a Mac and Ctrl elsewhere, and nothing else may be held", () => {
    expect(matchesKey("Mod-i", ev("i", { metaKey: true }), true)).toBe(true);
    expect(matchesKey("Mod-i", ev("i", { ctrlKey: true }), true)).toBe(false);
    expect(matchesKey("Mod-i", ev("i", { ctrlKey: true }), false)).toBe(true);
    expect(matchesKey("Mod-i", ev("i", { ctrlKey: true, shiftKey: true }), false)).toBe(false);
    expect(matchesKey("Mod-i", ev("i"), false)).toBe(false);
  });
  it("named modifiers and keys", () => {
    expect(matchesKey("Mod-Shift-Enter", ev("Enter", { ctrlKey: true, shiftKey: true }), false)).toBe(true);
    expect(matchesKey("Alt-s", ev("s", { altKey: true }), true)).toBe(true);
    expect(matchesKey("Mod-i", ev("I", { ctrlKey: true }), false)).toBe(true);
  });
});
