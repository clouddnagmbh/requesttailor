// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { CollectionInfo, RecentCollection } from "../commands/backend";
import { MockBackend } from "../commands/mock";
import { MockRecent } from "../commands/mock-recent";
import { AppState } from "../state/app.svelte";
import { afterFirstFrame, reopenOnStart } from "./startup";

function backend(recent: RecentCollection[], open: (id: string, interactive: boolean) => CollectionInfo | null) {
  const b = new MockBackend();
  const calls: [string, boolean][] = [];
  b.recentCollections = async () => recent;
  b.openRecent = async (id, interactive) => {
    calls.push([id, interactive]);
    return open(id, interactive);
  };
  return { b, calls };
}

const entry = (id: string): RecentCollection => ({ id, name: id, location: id, openedAt: 1 });

describe("reopenOnStart", () => {
  it("reopens the most recent collection without a prompt", async () => {
    const info = { id: "a", name: "A", location: "a" };
    const { b, calls } = backend([entry("a"), entry("b")], () => info);
    expect(await reopenOnStart(new AppState(b))).toEqual(info);
    expect(calls).toEqual([["a", false]]);
  });

  it("shows the start screen when nothing was opened before", async () => {
    const { b, calls } = backend([], () => null);
    expect(await reopenOnStart(new AppState(b))).toBeNull();
    expect(calls).toEqual([]);
  });

  it("shows the start screen when the platform needs consent or the folder is gone", async () => {
    const { b, calls } = backend([entry("a")], () => null);
    expect(await reopenOnStart(new AppState(b))).toBeNull();
    expect(calls).toEqual([["a", false]]);
  });

  it("leaves a collection alone that the user opened meanwhile", async () => {
    const { b, calls } = backend([entry("a")], () => ({ id: "a", name: "A", location: "a" }));
    const app = new AppState(b);
    const pending = reopenOnStart(app);
    await app.open();
    expect(await pending).toBeNull();
    expect(calls).toEqual([]);
  });

  it("the mock backend reopens what it opened", async () => {
    const b = new MockBackend();
    const info = await b.openCollection();
    expect((await b.recentCollections()).map((r) => r.id)).toEqual([info!.id]);
    expect(await reopenOnStart(new AppState(b))).toEqual(info);
  });
});

describe("afterFirstFrame", () => {
  it("waits for an animation frame, then for a task", async () => {
    const frames: (() => void)[] = [];
    const g = globalThis as { requestAnimationFrame?: (cb: () => void) => number };
    g.requestAnimationFrame = (cb) => frames.push(cb);
    try {
      let done = false;
      const p = afterFirstFrame().then(() => (done = true));
      await new Promise((r) => setTimeout(r, 20));
      expect(done).toBe(false);
      expect(frames).toHaveLength(1);
      frames[0]();
      await p;
      expect(done).toBe(true);
    } finally {
      delete g.requestAnimationFrame;
    }
  });

  it("resolves without animation frames (no DOM)", async () => {
    await afterFirstFrame();
  });
});

describe("MockRecent", () => {
  it("keeps the last opened first, without duplicates", () => {
    const r = new MockRecent();
    r.opened({ id: "a", name: "A", location: "a" });
    r.opened({ id: "b", name: "B", location: "b" });
    r.opened({ id: "a", name: "A", location: "a" });
    expect(r.list().map((e) => e.id)).toEqual(["a", "b"]);
    expect(typeof r.list()[0].openedAt).toBe("number");
  });
});
