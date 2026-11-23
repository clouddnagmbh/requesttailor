// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AppExtension } from "../extensions";
import { AppState } from "../state/app.svelte";
import { createExtensionContext } from "./extension-context";
import { dismissalKey, extensionHintsFor, extensionNodeActions, pickHint } from "./extension-hints";
import type { ProvidedExtensions } from "./extension-tabs";

function provided(extensions: AppExtension[]): ProvidedExtensions {
  const backend = new MockBackend();
  return { extensions, ctx: createExtensionContext(backend, new AppState(backend)) };
}

describe("extension hints and node actions (wave 14)", () => {
  const one: AppExtension = {
    id: "one",
    hints: (_ctx, target) => (target.place === "collection" ? [{ id: "tip", text: "A tip." }] : []),
    nodeActions: (_ctx, target) => (target.kind === "folder" ? [{ id: "open", label: "Open", inline: true, run: () => {} }] : []),
  };
  const broken: AppExtension = {
    id: "broken",
    hints: () => {
      throw new Error("no");
    },
    nodeActions: () => {
      throw new Error("no");
    },
  };

  it("keys everything by extension and skips extensions that throw", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const p = provided([broken, one, { id: "none" }]);
    expect(extensionHintsFor(p, { place: "collection", collection: "c" }).map((h) => h.key)).toEqual(["one/tip"]);
    expect(extensionNodeActions(p, { kind: "folder", collection: "c", path: "users" }).map((a) => a.key)).toEqual(["one/open"]);
    expect(extensionNodeActions(p, { kind: "collection", collection: "c" })).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(3);
    warn.mockRestore();
  });

  it("has nothing outside the shell", () => {
    expect(extensionHintsFor(null, { place: "collection", collection: "c" })).toEqual([]);
    expect(extensionNodeActions(null, { kind: "collection", collection: "c" })).toEqual([]);
  });

  describe("one hint per place", () => {
    const target = { place: "collection", collection: "c" } as const;
    const request = { place: "request", collection: "c", path: "a.yml" } as never;
    const x: AppExtension = {
      id: "x",
      hints: () => [
        { id: "first", text: "First." },
        { id: "second", text: "Second." },
      ],
    };
    const y: AppExtension = { id: "y", hints: () => [{ id: "only", text: "Y.", scope: "target" }] };

    it("shows the first hint that is not dismissed", () => {
      const refs = extensionHintsFor(provided([x, y]), target);
      expect(pickHint(refs, target, new Set())?.key).toBe("x/first");
      expect(pickHint(refs, target, new Set(["x/first"]))?.key).toBe("x/second");
      expect(pickHint(refs, target, new Set(["x/first", "x/second"]))?.key).toBe("y/only");
      expect(pickHint(refs, target, new Set(["x/first", "x/second", dismissalKey(refs[2]!, target)]))).toBeNull();
    });

    it("scope device is dismissed everywhere, scope target only for its collection or request", () => {
      const [first, , only] = extensionHintsFor(provided([x, y]), target);
      expect(dismissalKey(first!, target)).toBe("x/first");
      expect(dismissalKey(first!, request)).toBe("x/first");
      expect(dismissalKey(only!, target)).toBe("y/only@c");
      expect(dismissalKey(only!, request)).toBe("y/only@c:a.yml");
      expect(dismissalKey(only!, { place: "collection", collection: "d" })).not.toBe(dismissalKey(only!, target));
      expect(pickHint([only!], { place: "collection", collection: "d" }, new Set([dismissalKey(only!, target)]))?.key).toBe("y/only");
    });
  });
});
