// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E: the web shell's inbox over the companion's `/v1/inbox` socket, with a fake
// socket. The companion does the listening and the core makes the request.

import type { CapturedRequest, InboxDraft, InboxStatus } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { webInbox, type SocketLike } from "./inbox";

class FakeSocket implements SocketLike {
  readyState = 0;
  onopen: ((e: unknown) => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onclose: ((e: unknown) => void) | null = null;
  onerror: ((e: unknown) => void) | null = null;
  sent: Record<string, unknown>[] = [];
  /** What the companion answers to a message. */
  reply: (m: Record<string, unknown>) => Record<string, unknown>[] = () => [];
  constructor(readonly url: string) {
    queueMicrotask(() => {
      this.readyState = 1;
      this.onopen?.({});
      this.push({ op: "status", status: OFF });
    });
  }
  push(message: Record<string, unknown>) {
    this.onmessage?.({ data: JSON.stringify(message) });
  }
  send(data: string) {
    const m = JSON.parse(data) as Record<string, unknown>;
    this.sent.push(m);
    queueMicrotask(() => this.reply(m).forEach((r) => this.push(r)));
  }
  close() {
    this.readyState = 3;
    this.onclose?.({});
  }
}

const OFF: InboxStatus = { running: false, held: 0, dropped: 0 };
const ON: InboxStatus = { running: true, port: 47855, url: "http://127.0.0.1:47855/", held: 0, dropped: 0 };
const req = (id: number): CapturedRequest => ({ id, at: 1, method: "POST", target: "/hook", headers: [["X-A", "b"]], body: "x", size: 1 });
const CANARY_DRAFT: InboxDraft = { name: "POST /hook", spec: {} as InboxDraft["spec"], secrets: [] };

function setup(paired = true) {
  const sockets: FakeSocket[] = [];
  const made: CapturedRequest[] = [];
  const api = webInbox({
    credentials: async () => (paired ? { url: "http://127.0.0.1:47812", token: "tok en" } : null),
    core: async () => ({
      inboxMakeRequest: (r) => {
        made.push(r);
        return CANARY_DRAFT;
      },
    }),
    webSocket: (url) => {
      const s = new FakeSocket(url);
      s.reply = (m) => (m.op === "start" ? [{ op: "status", status: ON }] : m.op === "stop" ? [{ op: "status", status: OFF }] : [{ op: "status", status: OFF }]);
      sockets.push(s);
      return s;
    },
  });
  return { api, sockets, made };
}

describe("web inbox", () => {
  it("connects to the companion with the token in the query and starts", async () => {
    const { api, sockets } = setup();
    const status = await api.start();
    expect(status.running).toBe(true);
    expect(sockets[0].url).toBe("ws://127.0.0.1:47812/v1/inbox?token=tok%20en");
    expect(sockets[0].sent[0]).toMatchObject({ op: "start", settings: { port: 47855, status: 200 } });
  });

  it("mirrors the events and serves events(since) and get", async () => {
    const { api, sockets } = setup();
    await api.start();
    sockets[0].push({ op: "event", event: { kind: "started", port: 47855 } });
    sockets[0].push({ op: "event", event: { kind: "request", request: req(1) } });
    sockets[0].push({ op: "event", event: { kind: "request", request: req(2) } });
    const page = await api.events(0);
    expect(page.events.map((e) => e.kind)).toEqual(["started", "request", "request"]);
    expect((await api.events(page.next)).events).toEqual([]);
    expect((await api.events(2)).events).toHaveLength(1);
    expect((await api.get(2))?.id).toBe(2);
    expect(await api.get(9)).toBeNull();
  });

  it("makes the request in the core", async () => {
    const { api, sockets, made } = setup();
    await api.start();
    sockets[0].push({ op: "event", event: { kind: "request", request: req(1) } });
    expect(await api.makeRequest(1)).toBe(CANARY_DRAFT);
    expect(made[0].id).toBe(1);
    await expect(api.makeRequest(5)).rejects.toThrow(/no longer held/);
  });

  it("reports a failure of the companion (a port in use)", async () => {
    const { api, sockets } = setup();
    await api.status();
    sockets[0].reply = () => [{ op: "failed", message: "Port 47855 is already in use. Pick another port." }];
    await expect(api.start()).rejects.toThrow(/in use/);
  });

  it("is quietly off without a paired companion and explains on start", async () => {
    const { api, sockets } = setup(false);
    expect((await api.status()).running).toBe(false);
    expect(sockets).toHaveLength(0);
    await expect(api.start()).rejects.toThrow(/rt agent/);
  });

  it("clears and stops over the socket", async () => {
    const { api, sockets } = setup();
    await api.start();
    await api.clear();
    expect(sockets[0].sent.at(-1)).toEqual({ op: "clear" });
    expect((await api.stop()).running).toBe(false);
  });

  it("a reconnect starts the list over", async () => {
    const { api, sockets } = setup();
    await api.start();
    sockets[0].push({ op: "event", event: { kind: "request", request: req(1) } });
    sockets[0].close();
    await api.start();
    const kinds = (await api.events(0)).events.map((e) => e.kind);
    expect(kinds).toContain("cleared");
    expect(sockets).toHaveLength(2);
  });
});
