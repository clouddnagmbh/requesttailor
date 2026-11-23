// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AppExtension } from "../extensions";
import { AppState } from "../state/app.svelte";
import { extensionHintsFor, extensionNodeActions } from "./extension-hints";
import type { ProvidedExtensions } from "./extension-tabs";

function provided(extensions: AppExtension[]): ProvidedExtensions {
  const backend = new MockBackend();
  return { extensions, ctx: { backend, app: new AppState(backend) } };
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
});
