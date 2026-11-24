// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C: a pasted cURL command or share link is detected, offered, and taken
// without any request being made: the cURL command is read by the core's importer (the mock
// stands in for it), a link opens as an unsaved draft.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { AppState } from "../state/app.svelte";
import { classifyPaste, looksShared } from "../share/detect";
import { clearSharedInbox, sharedInbox } from "../share/inbox.svelte";
import { askCurl, hintText, offerFor, takeOffer } from "./paste-curl";

const CURL = "curl -X POST https://api.test/items -H 'Content-Type: application/json' -d '{\"a\":1}'";

describe("what a paste is", () => {
  it("knows a cURL command", () => {
    expect(classifyPaste(CURL)).toBe("curl");
    expect(classifyPaste("  $ curl -s https://x.test")).toBe("curl");
    expect(classifyPaste("curling is a sport")).toBeNull();
    expect(classifyPaste("https://api.test/items")).toBeNull();
  });

  it("knows a link, a fragment and an rt: payload", () => {
    for (const text of [
      "https://app.example.test/#rt=1.abc_-DEF",
      "#rt=1.abc",
      "rt=1.abc",
      "rt:1.abc",
      "  rt:1.abc\n",
      "https://x.test/#a=1&rt=1.abc",
    ]) {
      expect(looksShared(text), text).toBe(true);
      expect(classifyPaste(text), text).toBe("share");
    }
    for (const text of ["", "rt:", "rt:abc", "rt:1.", "https://x.test/#section", "https://x.test/?rt=1.abc", "hello"]) {
      expect(looksShared(text), text).toBe(false);
    }
  });

  it("offers only for something it can take", () => {
    expect(offerFor("hello")).toBeNull();
    expect(offerFor(CURL)).toEqual({ kind: "curl", text: CURL });
    expect(hintText({ kind: "curl", text: CURL }, false)).toBe("This looks like a cURL command. Turn it into a request");
    expect(hintText({ kind: "curl", text: CURL }, true)).toContain("Replace this request");
    expect(hintText({ kind: "share", text: "rt:1.a" }, false)).toContain("shared request");
  });
});

async function opened() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  await app.select("users/create-user.yml");
  return { app, backend };
}

describe("taking a pasted cURL command", () => {
  it("replaces the request, keeps its name, and stays unsaved", async () => {
    const { app } = await opened();
    const before = app.drafts["users/create-user.yml"];
    const note = await takeOffer(app, "users/create-user.yml", { kind: "curl", text: CURL });
    const spec = app.drafts["users/create-user.yml"];
    expect(spec.method).toBe("POST");
    expect(spec.url).toBe("https://api.test/items");
    expect(spec.name).toBe(before.name);
    expect(note).toContain("cURL");
    expect(app.dirty).toContain("users/create-user.yml");
  });

  it("makes a new request from it", async () => {
    const { app } = await opened();
    const note = await askCurl(app, CURL);
    expect(app.active).not.toBe("users/create-user.yml");
    expect(app.drafts[app.active!].url).toBe("https://api.test/items");
    expect(note).toContain("cURL");
  });
});

describe("opening a pasted link", () => {
  it("opens it as an unsaved draft, sends nothing and says what it needs", async () => {
    const { app, backend } = await opened();
    const sent: unknown[] = [];
    const send = backend.send.bind(backend);
    backend.send = async (...args: Parameters<typeof send>) => (sent.push(args), send(...args));
    const link = await backend.share.link("users/create-user.yml", { include_values: false });
    await app.openShared(link.text);
    expect(sent).toEqual([]);
    const path = app.active!;
    expect(path).not.toBe("users/create-user.yml");
    expect(app.drafts[path].url).toBe(link.payload.request.url);
    expect(app.dirty).toContain(path);
    expect(sharedInbox.opened?.path).toBe(path);
    expect(sharedInbox.offer).toBeNull();
    clearSharedInbox();
  });

  it("refuses text that is no link, with a reason", async () => {
    const { app } = await opened();
    await expect(app.openShared("rt:1.!!!")).rejects.toMatchObject({ kind: "format" });
  });
});
