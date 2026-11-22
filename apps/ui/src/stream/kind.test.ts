// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { emptyRequest } from "../commands/mock";
import type { RequestSpec, StreamEvent } from "../commands/types";
import { asKind, filterEvents, formatAt, messagesOf, prettyJson, selectedMessage, sessionKind, summary } from "./kind";

const http = (headers: RequestSpec["headers"] = []): RequestSpec => ({ ...emptyRequest("r"), headers });
const decode = (f: { data: string }) => f.data;

describe("sessionKind", () => {
  it("is null for plain HTTP", () => {
    expect(sessionKind(http())).toBeNull();
    expect(sessionKind(undefined)).toBeNull();
    expect(sessionKind(http([{ name: "Accept", value: "application/json", enabled: true }]))).toBeNull();
  });

  it("reads WebSocket and gRPC from the protocol", () => {
    expect(sessionKind(asKind(http(), "websocket"))).toBe("websocket");
    expect(sessionKind(asKind(http(), "grpc"))).toBe("grpc");
    expect(sessionKind(asKind(http(), "graphql"))).toBeNull();
  });

  it("reads SSE from an enabled Accept header, case-insensitively", () => {
    expect(sessionKind(http([{ name: "accept", value: "Text/Event-Stream", enabled: true }]))).toBe("sse");
    expect(sessionKind(http([{ name: " ACCEPT ", value: "text/event-stream, */*", enabled: true }]))).toBe("sse");
    expect(sessionKind(http([{ name: "Accept", value: "text/event-stream", enabled: false }]))).toBeNull();
    expect(sessionKind(http([{ name: "X-Accept", value: "text/event-stream", enabled: true }]))).toBeNull();
  });
});

describe("new request kinds", () => {
  it("builds the specs the format writes", () => {
    const ws = asKind(http(), "websocket");
    expect(ws.method).toBe("WS");
    expect(ws.protocol).toEqual({
      type: "websocket",
      messages: [{ title: "", selected: true, kind: "text", data: "" }],
      timeout_ms: null,
      keep_alive_ms: null,
    });
    const grpc = asKind(http(), "grpc");
    expect(grpc.method).toBe("GRPC");
    expect(grpc.protocol).toMatchObject({ type: "grpc", method: "", method_type: "unary", proto_file: null });
    const gql = asKind(http(), "graphql");
    expect(gql.method).toBe("POST");
    expect(gql.body).toEqual({ type: "graphql", query: "", variables: "" });
    expect(gql.protocol).toBeUndefined();
    expect(asKind(http(), "http")).toEqual(http());
  });
});

describe("messages", () => {
  it("picks the selected variant, else the first", () => {
    const ws = asKind(http(), "websocket");
    expect(messagesOf(ws)).toHaveLength(1);
    expect(messagesOf(http())).toEqual([]);
    const a = { title: "a", selected: false, kind: "text" as const, data: "1" };
    const b = { ...a, title: "b", selected: true };
    expect(selectedMessage([a, b])?.title).toBe("b");
    expect(selectedMessage([a])?.title).toBe("a");
    expect(selectedMessage([])).toBeNull();
  });
});

describe("history helpers", () => {
  it("formats times since opening", () => {
    expect(formatAt(0)).toBe("00:00.000");
    expect(formatAt(61_005)).toBe("01:01.005");
    expect(formatAt(3_600_000 + 2_000)).toBe("1:00:02.000");
  });

  it("pretty-prints JSON objects and arrays only", () => {
    expect(prettyJson('{"a":1}')).toBe('{\n  "a": 1\n}');
    expect(prettyJson("[1]")).toBe("[\n  1\n]");
    expect(prettyJson("42")).toBeNull();
    expect(prettyJson("{nope")).toBeNull();
  });

  it("filters by text, name and id", () => {
    const events: StreamEvent[] = [
      { event: "opened", at_ms: 0, head: { status: 101, version: "HTTP/1.1", headers: [], url: "ws://x" } },
      { event: "message", at_ms: 1, direction: "sent", frame: { type: "text", data: "hello" } },
      { event: "message", at_ms: 2, direction: "received", frame: { type: "text", data: "{}" }, name: "tick", id: "7" },
      { event: "closed", at_ms: 3, code: 1000, reason: "bye", by_client: false },
    ];
    expect(filterEvents(events, "", decode)).toEqual([0, 1, 2, 3]);
    expect(filterEvents(events, "HELLO", decode)).toEqual([1]);
    expect(filterEvents(events, "tick", decode)).toEqual([2]);
    expect(filterEvents(events, "7", decode)).toEqual([2]);
    expect(filterEvents(events, "bye", decode)).toEqual([3]);
    expect(summary(events[3], decode)).toBe("Closed by server (1000): bye");
  });
});
