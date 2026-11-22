// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { fuzzyFilter, fuzzyScore } from "./fuzzy";
import { formatShortcut, isMacPlatform, matchShortcut, SHORTCUTS } from "./shortcuts";
import { requestPaths, typeahead, visibleRows } from "./tree";

const key = (k: string, mods: Partial<Record<"metaKey" | "ctrlKey" | "altKey" | "shiftKey", boolean>> = {}) => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  shiftKey: false,
  ...mods,
});

describe("shortcuts", () => {
  it("uses Cmd on macOS and Ctrl elsewhere", () => {
    expect(matchShortcut(key("s", { metaKey: true }), true)?.command).toBe("save");
    expect(matchShortcut(key("s", { ctrlKey: true }), true)).toBeUndefined();
    expect(matchShortcut(key("S", { ctrlKey: true }), false)?.command).toBe("save");
    expect(matchShortcut(key("Enter", { ctrlKey: true }), false)?.command).toBe("send");
    expect(matchShortcut(key("s", { ctrlKey: true, shiftKey: true }), false)).toBeUndefined();
    expect(matchShortcut(key("s"), false)).toBeUndefined();
  });

  it("formats the body with Shift+Alt+F (by key code on macOS, where Alt changes the key)", () => {
    const shiftAlt = { shiftKey: true, altKey: true };
    expect(matchShortcut({ ...key("Ï", shiftAlt), code: "KeyF" }, true)?.command).toBe("format-body");
    expect(matchShortcut({ ...key("F", shiftAlt), code: "KeyF" }, false)?.command).toBe("format-body");
    expect(matchShortcut(key("F", shiftAlt), false)?.command).toBe("format-body");
    expect(matchShortcut({ ...key("f", { ctrlKey: true }), code: "KeyF" }, false)).toBeUndefined();
    expect(matchShortcut({ ...key("F", { ...shiftAlt, ctrlKey: true }), code: "KeyF" }, false)).toBeUndefined();
    expect(matchShortcut({ ...key("S", shiftAlt), code: "KeyS" }, false)).toBeUndefined();
    expect(formatShortcut({ command: "format-body", key: "f", shiftAlt: true }, true)).toBe("⇧⌥F");
    expect(formatShortcut({ command: "format-body", key: "f", shiftAlt: true }, false)).toBe("Shift+Alt+F");
  });

  it("has one entry per key and formats per platform", () => {
    expect(new Set(SHORTCUTS.map((s) => s.key)).size).toBe(SHORTCUTS.length);
    expect(new Set(SHORTCUTS.map((s) => s.command)).size).toBe(SHORTCUTS.length);
    expect(formatShortcut({ command: "palette", key: "k" }, true)).toBe("⌘K");
    expect(formatShortcut({ command: "send", key: "Enter" }, false)).toBe("Ctrl+Enter");
    expect(isMacPlatform({ platform: "MacIntel" })).toBe(true);
    expect(isMacPlatform({ platform: "Win32" })).toBe(false);
  });
});

describe("fuzzy", () => {
  it("matches subsequences per word and ranks word starts first", () => {
    expect(fuzzyScore("sndreq", "Send request")).not.toBeNull();
    expect(fuzzyScore("xyz", "Send request")).toBeNull();
    expect(fuzzyScore("env prd", "Environment: PRD")).not.toBeNull();
    expect(fuzzyScore("env prd", "Environment: Local")).toBeNull();
    const labels = ["Close tab", "Save", "Send request", "Toggle theme"];
    expect(fuzzyFilter(labels, "s", (x) => x)[0]).toBe("Save");
    expect(fuzzyFilter(labels, "", (x) => x)).toEqual(labels);
  });
});

describe("tree rows", () => {
  it("flattens visible rows and skips collapsed folders", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const tree = await b.tree();
    const rows = visibleRows(tree, new Set());
    expect(rows.map((r) => `${r.level}:${r.node.name}`)).toEqual([
      "1:demo",
      "2:Blocked by the browser",
      "2:100 MB response",
      "1:Health",
      "1:users",
      "2:Create user",
      "2:List users",
    ]);
    expect(rows[1].parent).toBe("f:demo");
    const collapsed = visibleRows(tree, new Set(["users"]));
    expect(collapsed.map((r) => r.node.name)).not.toContain("List users");
    expect(requestPaths(tree)).toHaveLength(5);
    expect(rows[typeahead(rows, 0, "h")].node.name).toBe("Health");
    expect(typeahead(rows, 0, "q")).toBe(0);
  });
});
