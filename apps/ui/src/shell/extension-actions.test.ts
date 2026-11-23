// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AppExtension, EditorTarget } from "../extensions";
import { AppState } from "../state/app.svelte";
import {
  extensionAssistant,
  extensionEditorActions,
  extensionRangeActions,
  extensionRequestActions,
  extensionResponseActions,
  extensionRunActions,
} from "./extension-actions";
import { createExtensionAi } from "../ai/extension-ai";
import { createExtensionContext } from "./extension-context";
import type { ProvidedExtensions } from "./extension-tabs";

function provided(extensions: AppExtension[]): ProvidedExtensions {
  const backend = new MockBackend();
  return { extensions, ctx: createExtensionContext(backend, new AppState(backend)) };
}

const script: EditorTarget = { kind: "script", collection: "c", owner: "request", path: "a.yml", phase: "tests" };

describe("extension actions and the assistant panel (wave 16 interface)", () => {
  const one: AppExtension = {
    id: "one",
    editorActions: (_ctx, target) => (target.kind === "script" ? [{ id: "write", label: "Write with AI", run: () => {} }] : []),
    requestActions: () => [{ id: "r", label: "R", run: () => {} }],
    responseActions: (_ctx, target) => (target.failed ? [{ id: "explain", label: "Explain", inline: true, run: () => {} }] : []),
    runActions: (_ctx, target) => (target.kind === "bisect" ? [{ id: "why", label: "Why?", run: () => {} }] : []),
    rangeActions: (_ctx, target) => (target.source === "graph" ? [{ id: "notes", label: "Release notes", run: () => {} }] : []),
  };
  const broken: AppExtension = {
    id: "broken",
    editorActions: () => {
      throw new Error("no");
    },
  };

  it("collects per place, keyed by extension, and skips an extension that throws", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const p = provided([broken, one]);
    expect(extensionEditorActions(p, script).map((a) => a.key)).toEqual(["one/write"]);
    expect(extensionEditorActions(p, { kind: "body", collection: "c", path: "a.yml", contentType: null })).toEqual([]);
    expect(extensionRequestActions(p, { collection: "c", path: "a.yml", spec: {} as never }).map((a) => a.key)).toEqual(["one/r"]);
    expect(extensionResponseActions(p, { collection: "c", path: "a.yml", response: {} as never, failed: true })).toHaveLength(1);
    expect(extensionRunActions(p, { kind: "bisect", collection: "c", outcome: { first_bad: null, candidates: 0, steps: [] } })).toHaveLength(1);
    expect(extensionRangeActions(p, { collection: null, from: "v1", to: "v2", source: "graph" }).map((a) => a.key)).toEqual(["one/notes"]);
    expect(extensionEditorActions(null, script)).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(2); // once per editorActions call above
    warn.mockRestore();
  });

  it("shows the first extension's assistant panel", () => {
    expect(extensionAssistant(provided([one]))).toBeNull();
    const panel = { title: "Assistant", component: (() => {}) as never };
    expect(extensionAssistant(provided([one, { id: "a", assistant: panel }, { id: "b", assistant: panel }]))?.extension).toBe("a");
  });

  it("gives extensions the AI provider (off until set up) and the suggestion dialog", async () => {
    const p = provided([]);
    const status = await p.ctx.ai.status();
    expect(status).toMatchObject({ configured: false, allowed: true, profiles: [] });
    await expect(p.ctx.ai.complete({ purpose: "x", system: "", messages: [] })).rejects.toMatchObject({ kind: "not-configured" });
    // `ctx.suggest` is built (strand C): a `*.yml` file is refused up front, before any dialog.
    await expect(p.ctx.suggest({ kind: "file", path: "a/b.yml", text: "" })).rejects.toMatchObject({ kind: "invalid" });
  });

  describe("ctx.ai: preview, policy and abort", () => {
    const preview = { profile: "p", provider: "anthropic", model: "m", endpoint: "https://x.test", call: { system: "", messages: [] }, masked: 0, bytes: 1, required: true };
    function setup(policy = { file_off: false, device_off: false }) {
      const backend = new MockBackend();
      const calls: string[] = [];
      const cancelled: string[] = [];
      let finish: (() => void) | null = null;
      Object.assign(backend.ai, {
        policy: async () => policy,
        preview: async () => preview,
        confirmPreview: async () => {},
        cancel: async (id: string) => void cancelled.push(id),
        complete: (r: { id: string }) =>
          new Promise((resolve) => {
            calls.push(r.id);
            finish = () => resolve({ text: "ok", stop: "end" });
          }),
      });
      return { backend, calls, cancelled, finish: () => finish?.() };
    }
    const request = { purpose: "x", system: "", messages: [] };

    it("an opted-out collection refuses before anything is built", async () => {
      const t = setup({ file_off: true, device_off: false });
      const ai = createExtensionAi(t.backend, new AppState(t.backend), async () => true);
      await expect(ai.complete(request)).rejects.toMatchObject({ kind: "opted-out" });
      expect(t.calls).toEqual([]);
    });

    it("cancelling the preview sends nothing", async () => {
      const t = setup();
      const ai = createExtensionAi(t.backend, new AppState(t.backend), async () => false);
      await expect(ai.complete(request, { preview: true })).rejects.toMatchObject({ kind: "cancelled" });
      expect(t.calls).toEqual([]);
    });

    it("an aborted signal cancels the running call by its id", async () => {
      const t = setup();
      const ai = createExtensionAi(t.backend, new AppState(t.backend), async () => true);
      const controller = new AbortController();
      const done = ai.complete(request, { signal: controller.signal });
      await vi.waitFor(() => expect(t.calls).toHaveLength(1));
      controller.abort();
      await vi.waitFor(() => expect(t.cancelled).toEqual(t.calls));
      t.finish();
      await done;
    });
  });
});
