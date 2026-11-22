// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import type { StreamEvent } from "../commands/types";
import { AppState } from "../state/app.svelte";
import { asKind } from "./kind";
import { SessionsState } from "./sessions.svelte";

const settle = () => new Promise((r) => setTimeout(r, 120));

async function setup(kind: "websocket" | "sse") {
  const backend = new MockBackend();
  const app = new AppState(backend);
  await app.adopt((await backend.openCollection())!);
  await app.create("", kind === "websocket" ? "Echo" : "Events");
  const path = app.active!;
  let spec = asKind(app.drafts[path], kind === "websocket" ? "websocket" : "http");
  if (kind === "sse") spec = { ...spec, headers: [{ name: "Accept", value: "text/event-stream", enabled: true }] };
  if (spec.protocol?.type === "websocket") spec.protocol.messages[0].data = '{"hi":1}';
  await app.edit(path, spec);
  return { backend, app, sessions: new SessionsState(app), path };
}

const kinds = (events: StreamEvent[]) =>
  events.map((e) => (e.event === "message" ? `${e.direction}:${e.frame.data}` : e.event));

describe("sessions (mock backend)", () => {
  it("connects, echoes the selected message and disconnects", async () => {
    const { sessions, path } = await setup("websocket");
    expect(sessions.kindOf(path)).toBe("websocket");
    await sessions.primary(path);
    await settle();
    const entry = sessions.entries[path];
    expect(entry.status).toBe("open");
    // Connected: the primary action sends.
    await sessions.primary(path);
    await settle();
    await sessions.disconnect(path);
    await settle();
    expect(kinds(entry.events)).toEqual(["opened", 'sent:{"hi":1}', 'received:{"hi":1}', "closed"]);
    const closed = entry.events.at(-1);
    expect(closed).toMatchObject({ event: "closed", code: 1000, by_client: true });
    expect(entry.status).toBe("closed");
  });

  it("reads SSE events until the server closes", async () => {
    const { sessions, path, backend } = await setup("sse");
    await sessions.connect(path);
    await settle();
    const entry = sessions.entries[path];
    expect(kinds(entry.events)).toEqual(["opened", 'received:{"tick":1}', 'received:{"tick":2}', 'received:{"tick":3}', "closed"]);
    const log = await backend.streams.log(entry.id!);
    expect(log.kind).toBe("sse");
    expect(log.events).toHaveLength(5);
  });

  it("releases sessions of closed tabs", async () => {
    const { sessions, path, app, backend } = await setup("websocket");
    await sessions.connect(path);
    await settle();
    const id = sessions.entries[path].id!;
    app.close(path);
    await sessions.keepOnly(app.tabs);
    expect(sessions.entries[path]).toBeUndefined();
    await expect(backend.streams.log(id)).rejects.toThrow();
  });

  it("does not open HTTP requests", async () => {
    const backend = new MockBackend();
    const app = new AppState(backend);
    await app.adopt((await backend.openCollection())!);
    await app.select("health.yml");
    const sessions = new SessionsState(app);
    await sessions.connect("health.yml");
    expect(sessions.entries["health.yml"]).toBeUndefined();
    await expect(backend.streams.open("health.yml", { environment: null, vars: [] }, () => {})).rejects.toThrow();
  });
});
