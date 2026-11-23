// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand B: the app's own assistant tasks in the UI. Off until set up, off per
// collection, the preview before anything leaves, a cURL snippet without a model, and every
// proposal as a suggestion that is unsaved and (where it can be) undone. Mock backend only.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { AiPreview, AiSettings, RequestSpec } from "../commands/types";
import type { ExtensionDialog } from "../extensions";
import { allTiles } from "../new/tiles";
import { AppState } from "../state/app.svelte";
import type { Prepared } from "../suggest/apply";
import { aiFor } from "./ai.svelte";
import { aiCommands } from "./commands";
import { createFromText, draftDocs, explainResponse, suggestAssertions, suggestNames } from "./flows";
import { askTask, isLocal, looksLikeCurl, OFF_MESSAGE } from "./task";

const PATH = "users/list-users.yml";

const settings: AiSettings = {
  profiles: [{ id: "local", label: "Local", provider: "openai-compatible", model: "llama3.1", base_url: "http://localhost:11434/v1" }],
  default_profile: "local",
  roles: {},
  preview: "first-use",
};

/** An app whose dialogs are recorded; `answer(dialog)` is what the user does in each. */
async function setup(configured = true, answer: (d: ExtensionDialog) => unknown = () => undefined) {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  await app.select(PATH);
  if (configured) await backend.ai.setSettings(settings);
  const dialogs: ExtensionDialog[] = [];
  app.showDialog = async (d) => {
    dialogs.push(d);
    return answer(d);
  };
  const ai = aiFor(backend);
  await ai.reload();
  await ai.refreshPolicy();
  return { backend, app, ai, dialogs };
}

/** Confirms a preview, applies a suggestion; ignores the result dialogs unless `result` says. */
const accept =
  (result: (d: ExtensionDialog) => unknown = () => undefined) =>
  (d: ExtensionDialog) =>
    d.title === "What will be sent" ? { send: true } : d.title === "Review suggestion" ? true : result(d);

describe("what counts as a cURL snippet", () => {
  it("is a curl command line, with or without a shell prompt", () => {
    expect(looksLikeCurl("curl https://x.test")).toBe(true);
    expect(looksLikeCurl("  $ curl -s https://x.test")).toBe(true);
    expect(looksLikeCurl("curl")).toBe(true);
    expect(looksLikeCurl("curling is a sport")).toBe(false);
    expect(looksLikeCurl("fetch('https://x.test')")).toBe(false);
    expect(isLocal({ task: "request-from-snippet", snippet: "curl https://x.test", folder: "" })).toBe(true);
    expect(isLocal({ task: "create-request", description: "curl https://x.test", folder: "" })).toBe(false);
  });
});

describe("off until set up, off per collection", () => {
  it("offers nothing but the settings while the assistant is not set up", async () => {
    const { backend, app, ai } = await setup(false);
    expect(ai.ready).toBe(false);
    expect(aiCommands(app, ai).map((c) => c.id)).toEqual(["ai-settings"]);
    expect(allTiles([], true, ai.ready).some((t) => t.key === "ai-request")).toBe(false);
    // Nothing was asked, nothing is nagging.
    expect(backend.ai.asked).toEqual([]);
    expect(app.error).toBeNull();
  });

  it("without settings a task is refused before anything is built; a cURL snippet still works", async () => {
    const { backend, app, ai } = await setup(false);
    await expect(askTask(app, ai, { task: "explain-response", path: PATH, response: 1 })).rejects.toMatchObject({ kind: "not-configured" });
    expect(backend.ai.asked).toEqual([]);
    const answer = await askTask(app, ai, { task: "request-from-snippet", snippet: "curl -X DELETE https://api.test/items/1", folder: "" });
    expect(answer?.request).toMatchObject({ method: "DELETE", url: "https://api.test/items/1" });
    expect(backend.ai.sent).toEqual([]); // no model call, no preview
  });

  it("an opted-out collection refuses every task and says why", async () => {
    const { backend, app, ai } = await setup();
    backend.ai.fileOff = true;
    await ai.refreshPolicy();
    expect(ai.off).toBe(true);
    expect(ai.ready).toBe(false);
    expect(aiCommands(app, ai).map((c) => c.id)).toEqual(["ai-settings"]);
    expect(allTiles([], true, ai.ready).some((t) => t.key === "ai-request")).toBe(false);
    await expect(askTask(app, ai, { task: "suggest-names", target: "request", path: PATH })).rejects.toMatchObject({ kind: "opted-out" });
    expect(OFF_MESSAGE).toBe("AI is off for this collection");
    await explainResponse(app, ai, PATH, 1);
    expect(app.error).toBe("AI is off for this collection.");
    expect(backend.ai.asked).toEqual([]);
  });

  it("device opt-out works the same", async () => {
    const { backend, app, ai } = await setup();
    await backend.ai.setDeviceOptOut(true);
    await ai.refreshPolicy();
    expect(ai.off).toBe(true);
    await expect(askTask(app, ai, { task: "draft-docs", path: PATH })).rejects.toMatchObject({ kind: "opted-out" });
  });

  it("once set up, the palette and the New dialog offer the tasks", async () => {
    const { app, ai } = await setup();
    expect(ai.ready).toBe(true);
    const ids = aiCommands(app, ai).map((c) => c.id);
    expect(ids).toEqual(expect.arrayContaining(["ai-create", "ai-name-request", "ai-name-variables", "ai-docs", "ai-name-folder"]));
    const tiles = allTiles([], true, ai.ready);
    expect(tiles.find((t) => t.key === "ai-request")).toMatchObject({ title: "From description or snippet", group: "Requests" });
    expect(allTiles([], false, true).find((t) => t.key === "ai-request")?.disabled).toBeTruthy();
  });
});

describe("the preview comes first", () => {
  it("shows the masked call on first use, remembers the confirmation, and sends nothing on cancel", async () => {
    const { backend, app, ai, dialogs } = await setup(true, (d) => (d.title === "What will be sent" ? { send: false } : undefined));
    backend.ai.secrets = ["hunter2-secret"];
    const task = { task: "request-from-snippet", snippet: "GET /orders with password hunter2-secret", folder: "" } as const;
    expect(await askTask(app, ai, task)).toBeNull(); // cancelled
    expect(backend.ai.asked).toEqual([]);
    const preview = dialogs[0]!.props!.preview as AiPreview;
    expect(JSON.stringify(preview.call)).toContain("***");
    expect(JSON.stringify(preview.call)).not.toContain("hunter2-secret");
    expect(preview.required).toBe(true);

    app.showDialog = async (d) => {
      dialogs.push(d);
      return { send: true };
    };
    expect((await askTask(app, ai, task))?.request).toBeTruthy();
    expect(backend.ai.asked).toHaveLength(1);
    expect((await backend.ai.settings())?.previewed).toEqual(["local"]);
    // Confirmed once per profile and device: the second call goes without a dialog.
    const shown = dialogs.length;
    await askTask(app, ai, { task: "draft-docs", path: PATH });
    expect(dialogs.length).toBe(shown);
  });

  it("a cURL snippet shows no preview: nothing leaves", async () => {
    const { app, ai, dialogs, backend } = await setup();
    await askTask(app, ai, { task: "request-from-snippet", snippet: "curl https://x.test/a", folder: "" });
    expect(dialogs).toEqual([]);
    expect(backend.ai.sent).toEqual([]);
  });
});

async function review(dialogs: ExtensionDialog[]): Promise<Prepared> {
  const d = dialogs.find((x) => x.title === "Review suggestion");
  expect(d, "a suggestion was shown").toBeTruthy();
  return d!.props!.prepared as Prepared;
}

describe("proposals are suggestions: visible, unsaved, undoable", () => {
  it("assertions from the response", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept());
    const before = await backend.getRequest(PATH);
    expect(await suggestAssertions(app, ai, PATH, 1)).toBe(true);
    const p = await review(dialogs);
    expect(p.title).toBe("Change request");
    expect(p.blocks.map((b) => b.label).join(" ")).toContain("Assertion");
    expect(p.note?.title).toBe("Assertions from the response");
    // Nothing happened yet; the user applied it in the dialog (the test's `accept` only
    // answered). Applying is the dialog's `prepared.apply()`:
    expect((await backend.getRequest(PATH)).assertions).toEqual(before.assertions);
    const applied = await p.apply();
    const after = await backend.getRequest(PATH);
    expect(after.assertions.map((a) => a.expression)).toEqual(expect.arrayContaining(["res.status", "res.body.items"]));
    expect(await backend.dirty()).toContain(PATH); // unsaved
    expect(await applied.undo!()).toBe(true);
    expect((await backend.getRequest(PATH)).assertions).toEqual(before.assertions);
  });

  it("assertions the request already has are not added twice", async () => {
    const { backend, app, ai } = await setup(true, accept());
    const base = await backend.getRequest(PATH);
    await app.edit(PATH, {
      ...base,
      assertions: [
        { expression: "res.status", operator: "eq", value: "200", description: null, enabled: true },
        { expression: "res.body.items", operator: "isArray", value: "", description: null, enabled: true },
      ],
    });
    expect(await suggestAssertions(app, ai, PATH, 1)).toBe(false);
    expect(app.error).toContain("nothing to add");
  });

  it("a request from a cURL command: new request, unsaved, no model", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept());
    const ok = await createFromText(app, ai, {
      mode: "snippet",
      text: `curl -X POST https://api.test/orders -H 'Content-Type: application/json' -d '{"sku":"a-1"}'`,
      folder: "users",
    });
    expect(ok).toBe(true);
    const p = await review(dialogs);
    expect(p.title).toBe("New request");
    expect(p.blocks.some((b) => b.next === "POST")).toBe(true);
    await p.apply();
    expect(app.active).not.toBe(PATH);
    expect((await backend.getRequest(app.active!)).url).toBe("https://api.test/orders");
    expect((await backend.dirty()).length).toBeGreaterThan(0);
    expect(backend.ai.sent).toEqual([]);
  });

  it("a request from a description goes through the preview", async () => {
    const { app, ai, dialogs } = await setup(true, accept());
    expect(await createFromText(app, ai, { mode: "describe", text: "create an order", folder: "" })).toBe(true);
    expect(dialogs.map((d) => d.title)).toEqual(["What will be sent", "Review suggestion"]);
    expect((await review(dialogs)).blocks.length).toBeGreaterThan(0);
  });

  it("nothing proposed means the dialog can open again", async () => {
    const { app, ai } = await setup(true, (d) => (d.title === "What will be sent" ? { send: false } : undefined));
    expect(await createFromText(app, ai, { mode: "describe", text: "x", folder: "" })).toBe(false);
  });

  it("a request name is chosen, shown as a diff, applied and undone", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept((d) => (d.title === "Name this request" ? { name: "List open orders" } : undefined)));
    const before = await backend.getRequest(PATH);
    await suggestNames(app, ai, "request", PATH);
    const names = dialogs.find((d) => d.title === "Name this request")!;
    expect(names.props!.names).toEqual(["List open orders", "Open orders", "Get open orders"]);
    const p = await review(dialogs);
    expect(p.blocks).toEqual([expect.objectContaining({ label: "Name", old: before.name, next: "List open orders" })]);
    const applied = await p.apply();
    expect((await backend.getRequest(PATH)).name).toBe("List open orders");
    expect(await applied.undo!()).toBe(true);
    expect((await backend.getRequest(PATH)).name).toBe(before.name);
  });

  it("names of a folder or of variables are only offered for copying", async () => {
    const { app, ai, dialogs } = await setup(true, accept());
    await suggestNames(app, ai, "folder", "users");
    await suggestNames(app, ai, "variable", PATH);
    expect(dialogs.map((d) => d.title).filter((t) => t.startsWith("Name"))).toEqual(["Name this folder", "Name the variables"]);
    expect(dialogs.some((d) => d.title === "Review suggestion")).toBe(false);
  });

  it("a description draft is read first, then reviewed as a file, unsaved", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept((d) => (d.title === "Draft description" ? { save: true } : undefined)));
    await draftDocs(app, ai, PATH);
    const draft = dialogs.find((d) => d.title === "Draft description")!;
    expect(draft.props!.docs).toContain("open orders");
    const p = await review(dialogs);
    expect(p.subtitle).toBe("users/list-users.md");
    await p.apply();
    expect(await backend.companions.read("users/list-users.md")).toContain("open orders");
    expect(await backend.dirty()).toContain("users/list-users.md");
  });

  it("discarding a description draft changes nothing", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept());
    await draftDocs(app, ai, PATH);
    expect(dialogs.some((d) => d.title === "Review suggestion")).toBe(false);
    expect(await backend.companions.read("users/list-users.md")).toBeNull();
  });

  it("a response summary changes nothing", async () => {
    const { backend, app, ai, dialogs } = await setup(true, accept());
    const before: RequestSpec = await backend.getRequest(PATH);
    await explainResponse(app, ai, PATH, 1);
    expect(dialogs.map((d) => d.title)).toEqual(["What will be sent", "About this response"]);
    expect(await backend.getRequest(PATH)).toEqual(before);
    expect(await backend.dirty()).toEqual([]);
  });
});
