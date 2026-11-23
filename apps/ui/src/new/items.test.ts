// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: tiles of extensions and how the "New…" dialog is opened.

import { describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AppExtension } from "../extensions";
import type { ProvidedExtensions } from "../shell/extension-tabs";
import { AppState } from "../state/app.svelte";
import { createExtensionContext } from "../shell/extension-context";
import { extensionNewItems } from "./items";
import { isNewItemKind, NEW_ITEMS } from "./kinds";

function provided(extensions: AppExtension[]): ProvidedExtensions {
  const backend = new MockBackend();
  return { extensions, ctx: createExtensionContext(backend, new AppState(backend)) };
}

describe("New dialog (wave 14 interface)", () => {
  it("keys extension tiles by extension and skips extensions that throw", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const one: AppExtension = {
      id: "one",
      newItems: () => [{ id: "thing", title: "Thing", description: "A thing.", group: "More", run: () => {} }],
    };
    const broken: AppExtension = {
      id: "broken",
      newItems: () => {
        throw new Error("no");
      },
    };
    expect(extensionNewItems(provided([broken, one, { id: "none" }])).map((i) => i.key)).toEqual(["one/thing"]);
    expect(extensionNewItems(null)).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(1);
    warn.mockRestore();
  });

  it("has a tile for every built-in kind", () => {
    expect(NEW_ITEMS.map((i) => i.kind)).toEqual(["http", "graphql", "grpc", "websocket", "sse", "collection", "folder", "environment", "mcp-test"]);
    expect(isNewItemKind("sse")).toBe(true);
    expect(isNewItemKind("one/thing")).toBe(false);
  });

  it("opens for the active collection, or where it is asked for", async () => {
    const app = new AppState(new MockBackend());
    await app.open();
    app.openNew();
    expect(app.newDialog).toMatchObject({ collection: app.collection!.id, folder: "" });
    const first = app.newDialog!.seq;
    app.openNew({ kind: "folder", folder: "users" });
    expect(app.newDialog).toMatchObject({ kind: "folder", folder: "users" });
    expect(app.newDialog!.seq).toBeGreaterThan(first);
    app.closeNew();
    expect(app.newDialog).toBeNull();
  });
});
