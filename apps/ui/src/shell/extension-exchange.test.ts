// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AppExtension.onExchange` (wave 17): one event per send of the app, with the request as
// written (variables unresolved) and the stored response; errors stay with the extension; the
// collection's `observe.enabled: false` stops it.

import { describe, expect, it, vi } from "vitest";

import { MockBackend } from "../commands/mock";
import type { StoredResponse } from "../commands/types";
import type { AppExtension, ExchangeEvent } from "../extensions";
import { AppState } from "../state/app.svelte";
import { createExtensionContext } from "./extension-context";
import { connectExchange, deliverExchange } from "./extension-exchange";

async function setup() {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.open();
  await app.select("users/list-users.yml");
  return { backend, app, ctx: createExtensionContext(backend, app) };
}

describe("onExchange", () => {
  it("is called once per send with the request as written and the response", async () => {
    const { backend, app, ctx } = await setup();
    const seen: ExchangeEvent[] = [];
    const stop = connectExchange(app, [{ id: "rec", onExchange: (_c, e) => void seen.push(e) }], ctx);
    await app.send();
    await app.send();
    expect(seen).toHaveLength(2);
    const [first] = seen;
    expect(first.collection).toBe(app.collection!.id);
    expect(first.path).toBe("users/list-users.yml");
    expect(first.response.report.request_path).toBe("users/list-users.yml");
    // The request keeps its `{{variables}}` (the mock's list request uses the base URL).
    const written = await backend.getRequest("users/list-users.yml");
    expect(first.request).toEqual(written);
    expect(first.request.url).toContain("{{");
    expect(first.variables).toEqual(first.response.report.variables);
    stop();
    await app.send();
    expect(seen).toHaveLength(2);
  });

  it("uses the draft the user is editing (what the send used)", async () => {
    const { backend, app, ctx } = await setup();
    const seen: ExchangeEvent[] = [];
    connectExchange(app, [{ id: "rec", onExchange: (_c, e) => void seen.push(e) }], ctx);
    const base = await backend.getRequest("users/list-users.yml");
    await app.edit("users/list-users.yml", { ...base, url: "{{base}}/edited" });
    await app.send();
    expect(seen[0].request.url).toBe("{{base}}/edited");
  });

  it("is not called when the collection switched observers off", async () => {
    const { backend, app, ctx } = await setup();
    const seen: ExchangeEvent[] = [];
    connectExchange(app, [{ id: "rec", onExchange: (_c, e) => void seen.push(e) }], ctx);
    const real = backend.send.bind(backend);
    backend.send = async (path, input) => ({ ...(await real(path, input)), observe: false }) as StoredResponse;
    await app.send();
    expect(seen).toEqual([]);
  });

  it("an extension that throws neither stops the others nor the send", async () => {
    const { app, ctx } = await setup();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const ok = vi.fn();
    const broken: AppExtension = {
      id: "broken",
      onExchange: () => {
        throw new Error("no");
      },
    };
    connectExchange(app, [broken, { id: "ok", onExchange: ok }, { id: "none" }], ctx);
    await app.send();
    expect(ok).toHaveBeenCalledTimes(1);
    expect(app.results["users/list-users.yml"]).toBeDefined();
    expect(warn).toHaveBeenCalledWith(expect.stringContaining("broken"), expect.any(Error));
    warn.mockRestore();
  });

  it("reads nothing after a send when no extension wants the event", async () => {
    const { backend, app, ctx } = await setup();
    const spy = vi.spyOn(backend, "getRequest");
    const reads = async () => {
      spy.mockClear();
      await app.send();
      return spy.mock.calls.length;
    };
    const base = await reads();
    const stop = connectExchange(app, [{ id: "plain" }], ctx);
    expect(await reads()).toBe(base);
    stop();
  });

  it("deliverExchange hands the same event to every extension", () => {
    const events: ExchangeEvent[] = [];
    const ctx = {} as never;
    const event = { collection: "c" } as ExchangeEvent;
    deliverExchange([{ id: "a", onExchange: (_c, e) => void events.push(e) }, { id: "b", onExchange: (_c, e) => void events.push(e) }], ctx, event);
    expect(events).toEqual([event, event]);
  });
});
