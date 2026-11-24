// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SuggestedEdit.batch` (wave 17): several edits in one dialog, one undo step, applied entirely
// or not at all.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { RequestSpec } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { prepare } from "./apply";

const LIST = "users/list-users.yml";

async function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  await app.select(LIST);
  const tree = await backend.tree();
  const second = findRequests(tree).find((p) => p !== LIST)!;
  return { backend, app, second };
}

function findRequests(node: { path: string; kind: string; children?: unknown[] }): string[] {
  if (node.kind === "request") return [node.path];
  return ((node.children ?? []) as (typeof node)[]).flatMap(findRequests);
}

async function pair(backend: MockBackend, second: string) {
  const a = await backend.getRequest(LIST);
  const b = await backend.getRequest(second);
  const nextA: RequestSpec = { ...a, url: a.url + "?from=chain" };
  const nextB: RequestSpec = { ...b, headers: [...b.headers, { name: "X-Token", value: "{{token}}", enabled: true }] };
  return { a, b, nextA, nextB };
}

describe("batch suggestions", () => {
  it("shows both edits in one preview and applies them together with one undo", async () => {
    const { backend, app, second } = await setup();
    const { a, b, nextA, nextB } = await pair(backend, second);
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "request", path: LIST, spec: nextA },
        { kind: "request", path: second, spec: nextB },
      ],
      note: { title: "Chain the token" },
    });
    expect(p.title).toBe("Apply 2 changes");
    expect(p.note?.title).toBe("Chain the token");
    expect(p.blocks.map((x) => x.label)).toEqual(expect.arrayContaining([`${LIST} › URL`, `${second} › Header › X-Token`]));
    expect(p.undoable).toBe(true);

    const applied = await p.apply();
    expect((await backend.getRequest(LIST)).url).toBe(nextA.url);
    expect((await backend.getRequest(second)).headers.some((h) => h.name === "X-Token")).toBe(true);
    expect(await backend.dirty()).toEqual(expect.arrayContaining([LIST, second]));

    expect(await applied.undo!()).toBe(true);
    expect((await backend.getRequest(LIST)).url).toBe(a.url);
    expect((await backend.getRequest(second)).headers).toEqual(b.headers);
  });

  it("applies nothing when one part changed since the suggestion was made", async () => {
    const { backend, app, second } = await setup();
    const { a, nextA, nextB } = await pair(backend, second);
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "request", path: LIST, spec: nextA },
        { kind: "request", path: second, spec: nextB },
      ],
    });
    await app.edit(second, { ...(await backend.getRequest(second)), url: "https://typed.test/" });
    await expect(p.apply()).rejects.toMatchObject({ kind: "stale" });
    // The first part, which still matched, was not written either.
    expect((await backend.getRequest(LIST)).url).toBe(a.url);
  });

  it("puts back what was applied when a later part fails", async () => {
    const { backend, app, second } = await setup();
    const { a, nextA, nextB } = await pair(backend, second);
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "request", path: LIST, spec: nextA },
        { kind: "request", path: second, spec: nextB },
      ],
    });
    // The second write fails after the first went through.
    const real = app.applyExternal.bind(app);
    app.applyExternal = async (path, spec) => {
      if (path === second) throw new Error("disk is gone");
      return real(path, spec);
    };
    await expect(p.apply()).rejects.toThrow("disk is gone");
    expect((await backend.getRequest(LIST)).url).toBe(a.url);
  });

  it("does not undo if any part was changed afterwards", async () => {
    const { backend, app, second } = await setup();
    const { nextA, nextB } = await pair(backend, second);
    const applied = await (
      await prepare(backend, app, {
        kind: "batch",
        edits: [
          { kind: "request", path: LIST, spec: nextA },
          { kind: "request", path: second, spec: nextB },
        ],
      })
    ).apply();
    await app.edit(second, { ...(await backend.getRequest(second)), url: "https://later.test/" });
    expect(await applied.undo!()).toBe(false);
    // Neither part was reverted.
    expect((await backend.getRequest(LIST)).url).toBe(nextA.url);
    expect((await backend.getRequest(second)).url).toBe("https://later.test/");
  });

  it("mixes a request, folder scripts and a file, and refuses empty, nested and doubled batches", async () => {
    const { backend, app, second } = await setup();
    const { nextA } = await pair(backend, second);
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "request", path: LIST, spec: nextA },
        { kind: "folder-scripts", path: "users", scripts: [{ phase: "before-request", code: "// x" }] },
        { kind: "file", path: "notes/chain.md", text: "# Chain\n" },
      ],
    });
    expect(p.blocks.length).toBeGreaterThanOrEqual(3);
    await p.apply();
    expect(await backend.companions.read("notes/chain.md")).toBe("# Chain\n");

    await expect(prepare(backend, app, { kind: "batch", edits: [] })).rejects.toMatchObject({ kind: "invalid" });
    await expect(
      prepare(backend, app, { kind: "batch", edits: [{ kind: "batch", edits: [] } as never] }),
    ).rejects.toMatchObject({ kind: "invalid" });
    await expect(
      prepare(backend, app, {
        kind: "batch",
        edits: [
          { kind: "request", path: LIST, spec: nextA },
          { kind: "request", path: LIST, spec: nextA },
        ],
      }),
    ).rejects.toMatchObject({ kind: "invalid" });
  });

  it("a batch with a new request applies, but cannot be undone", async () => {
    const { backend, app, second } = await setup();
    const { nextA } = await pair(backend, second);
    const created = { ...nextA, name: "Fresh", url: "https://x.test/fresh" };
    const p = await prepare(backend, app, {
      kind: "batch",
      edits: [
        { kind: "new-request", folder: "users", spec: created },
        { kind: "request", path: LIST, spec: nextA },
      ],
    });
    expect(p.undoable).toBe(false);
    const applied = await p.apply();
    expect(applied.undo).toBeNull();
    expect((await backend.getRequest(LIST)).url).toBe(nextA.url);
  });
});
