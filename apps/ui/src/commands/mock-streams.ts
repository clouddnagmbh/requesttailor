// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `StreamsApi` of the mock backend (wave 8, strand A): a scripted echo for UI tests.
//   - WebSocket and gRPC: `opened` (101 / 200), then every sent message comes back as a
//     `message` "sent" followed by the same frame "received". The text `close` makes the
//     "server" close the session (1000, "bye").
//   - SSE (an HTTP request with `Accept: text/event-stream`): three events named `tick`
//     with ids 1 to 3, then the server closes.
//   - `close()`: `closed` 1000 by the client (SSE: no code).
// Events are delivered asynchronously, like a real transport.

import type { SendInput, StreamsApi } from "./backend";
import { BackendError } from "./backend";
import type { Frame, MessageVariant, RequestSpec, StreamEvent, StreamLog } from "./types";
import { sessionKind, type SessionKind } from "../stream/kind";

interface MockSession {
  path: string;
  kind: SessionKind;
  url: string;
  environment: string | null;
  events: StreamEvent[];
  onEvent: (event: StreamEvent) => void;
  started: number;
  open: boolean;
}

export interface MockStreamsDeps {
  getRequest: (path: string) => Promise<RequestSpec>;
}

const later = (ms = 0) => new Promise<void>((resolve) => setTimeout(resolve, ms));

function frameOf(message: MessageVariant): Frame {
  return message.kind === "binary" ? { type: "binary", data: message.data } : { type: "text", data: message.data };
}

export function mockStreams(deps: MockStreamsDeps): StreamsApi {
  const sessions = new Map<number, MockSession>();
  let nextId = 1;

  const at = (s: MockSession) => Math.round(performance.now() - s.started);
  const emit = (s: MockSession, event: StreamEvent) => {
    s.events.push(event);
    if (event.event === "closed" || event.event === "error") s.open = false;
    s.onEvent(event);
  };
  const get = (id: number) => {
    const s = sessions.get(id);
    if (!s) throw new BackendError("shell", `no session ${id}`);
    return s;
  };

  return {
    async open(path: string, input: SendInput, onEvent: (event: StreamEvent) => void): Promise<number> {
      const spec = await deps.getRequest(path);
      const kind = sessionKind(spec);
      if (!kind) throw new BackendError("unsupported", "this request is not a WebSocket, SSE or gRPC request");
      const id = nextId++;
      const s: MockSession = {
        path,
        kind,
        url: spec.url,
        environment: input.environment,
        events: [],
        onEvent,
        started: performance.now(),
        open: true,
      };
      sessions.set(id, s);
      void (async () => {
        await later();
        const status = kind === "websocket" ? 101 : 200;
        const headers: [string, string][] =
          kind === "websocket"
            ? [["upgrade", "websocket"]]
            : kind === "sse"
              ? [["content-type", "text/event-stream"]]
              : [["content-type", "application/grpc"]];
        emit(s, { event: "opened", at_ms: at(s), head: { status, version: "HTTP/1.1", headers, url: s.url } });
        if (kind !== "sse") return;
        for (let n = 1; n <= 3 && s.open; n++) {
          await later(20);
          if (!s.open) return;
          emit(s, {
            event: "message",
            at_ms: at(s),
            direction: "received",
            frame: { type: "text", data: JSON.stringify({ tick: n }) },
            name: "tick",
            id: String(n),
          });
        }
        if (s.open) emit(s, { event: "closed", at_ms: at(s), code: null, reason: "", by_client: false });
      })();
      return id;
    },

    async send(session: number, message: MessageVariant): Promise<void> {
      const s = get(session);
      if (s.kind === "sse") throw new BackendError("transport", "SSE sessions only receive");
      if (!s.open) throw new BackendError("transport", "the session is closed");
      const frame = frameOf(message);
      emit(s, { event: "message", at_ms: at(s), direction: "sent", frame });
      await later();
      if (!s.open) return;
      emit(s, { event: "message", at_ms: at(s), direction: "received", frame });
      if (frame.type === "text" && frame.data.trim() === "close") {
        emit(s, { event: "closed", at_ms: at(s), code: 1000, reason: "bye", by_client: false });
      }
    },

    async finish(session: number): Promise<void> {
      get(session);
    },

    async close(session: number): Promise<void> {
      const s = get(session);
      if (!s.open) return;
      emit(s, {
        event: "closed",
        at_ms: at(s),
        code: s.kind === "sse" ? null : 1000,
        reason: "",
        by_client: true,
      });
    },

    async log(session: number): Promise<StreamLog> {
      const s = get(session);
      return {
        version: 1,
        request_path: s.path,
        environment: s.environment,
        kind: s.kind,
        url: s.url,
        events: structuredClone(s.events),
      };
    },

    async release(session: number): Promise<void> {
      sessions.delete(session);
    },
  };
}
