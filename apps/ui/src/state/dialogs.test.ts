// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: main-area views and extension dialogs as `AppState` exposes them.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { ExtensionDialog } from "../extensions";
import { AppState } from "./app.svelte";

const component = (() => {}) as unknown as ExtensionDialog["component"];

describe("AppState views and dialogs (wave 14 interface)", () => {
  it("asks the shell for collection tabs and views", () => {
    const app = new AppState(new MockBackend());
    app.openCollectionTab("odd/diagram");
    expect(app.view?.view).toEqual({ kind: "collection-tab", key: "odd/diagram" });
    app.show({ kind: "runner", folder: null });
    expect(app.view?.view).toEqual({ kind: "runner", folder: null });
    app.openRequestTab("odd/query");
    expect(app.requestTab?.key).toBe("odd/query");
  });

  it("resolves a dialog with its result; a second one replaces the first", async () => {
    const app = new AppState(new MockBackend());
    const first = app.showDialog({ title: "One", component });
    const second = app.showDialog({ title: "Two", component });
    expect(await first).toBeUndefined();
    expect(app.dialog?.dialog.title).toBe("Two");
    app.closeDialog(42);
    expect(await second).toBe(42);
    expect(app.dialog).toBeNull();
  });

  it("keeps a running count of dialogs and ignores a close without a dialog", async () => {
    const app = new AppState(new MockBackend());
    expect(() => app.closeDialog("nothing")).not.toThrow();
    const a = app.showDialog({ title: "A", component, props: { n: 1 }, width: 720 });
    const seqA = app.dialog!.seq;
    expect(app.dialog?.dialog.props).toEqual({ n: 1 });
    expect(app.dialog?.dialog.width).toBe(720);
    app.closeDialog("a");
    expect(await a).toBe("a");
    void app.showDialog({ title: "B", component });
    expect(app.dialog!.seq).toBeGreaterThan(seqA);
  });

  it("closes one dialog once: a second close does not resolve it again", async () => {
    const app = new AppState(new MockBackend());
    const p = app.showDialog({ title: "A", component });
    app.closeDialog(1);
    app.closeDialog(2);
    expect(await p).toBe(1);
  });
});
