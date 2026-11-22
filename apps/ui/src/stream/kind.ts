// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Sessions in the UI (wave 8, strand A): which requests open a session instead of a send,
// their message variants, and the small formatting helpers of the message history.

import type { Frame, MessageVariant, RequestSpec, StreamEvent } from "../commands/types";

/** What a request opens instead of a plain HTTP send; `null`: a normal send. */
export type SessionKind = "websocket" | "grpc" | "sse";

/** WebSocket and gRPC by protocol; an HTTP request is read as Server-Sent Events when one of
 *  its enabled headers is `Accept` asking for `text/event-stream`. */
export function sessionKind(spec: RequestSpec | undefined | null): SessionKind | null {
  if (!spec) return null;
  const type = spec.protocol?.type;
  if (type === "websocket") return "websocket";
  if (type === "grpc") return "grpc";
  const sse = spec.headers.some(
    (h) => h.enabled && h.name.trim().toLowerCase() === "accept" && h.value.toLowerCase().includes("text/event-stream"),
  );
  return sse ? "sse" : null;
}

/** The message variants of a WebSocket or gRPC request (`[]` otherwise). */
export function messagesOf(spec: RequestSpec | undefined | null): MessageVariant[] {
  const p = spec?.protocol;
  return p && (p.type === "websocket" || p.type === "grpc") ? p.messages : [];
}

/** The variant to send: the selected one, else the first. */
export function selectedMessage(messages: MessageVariant[]): MessageVariant | null {
  return messages.find((m) => m.selected) ?? messages[0] ?? null;
}

export const emptyMessage = (title = ""): MessageVariant => ({ title, selected: true, kind: "text", data: "" });

/** The kinds the sidebar offers for a new request. */
export type NewRequestKind = "http" | "graphql" | "grpc" | "websocket";

export const NEW_REQUEST_KINDS: [NewRequestKind, string][] = [
  ["http", "HTTP"],
  ["graphql", "GraphQL"],
  ["grpc", "gRPC"],
  ["websocket", "WebSocket"],
];

/** `spec` (a fresh HTTP request) turned into a new request of `kind`. */
export function asKind(spec: RequestSpec, kind: NewRequestKind): RequestSpec {
  switch (kind) {
    case "http":
      return spec;
    case "graphql":
      return { ...spec, method: "POST", body: { type: "graphql", query: "", variables: "" } };
    case "websocket":
      return {
        ...spec,
        method: "WS",
        body: { type: "none" },
        protocol: { type: "websocket", messages: [emptyMessage()], timeout_ms: null, keep_alive_ms: null },
      };
    case "grpc":
      return {
        ...spec,
        method: "GRPC",
        body: { type: "none" },
        protocol: { type: "grpc", method: "", method_type: "unary", proto_file: null, messages: [{ ...emptyMessage(), kind: "json" }] },
      };
  }
}

/** `mm:ss.mmm` since the session opened (`h:mm:ss.mmm` after an hour). */
export function formatAt(ms: number): string {
  const total = Math.max(0, Math.floor(ms));
  const milli = String(total % 1000).padStart(3, "0");
  const s = Math.floor(total / 1000);
  const sec = String(s % 60).padStart(2, "0");
  const min = Math.floor(s / 60);
  if (min < 60) return `${String(min).padStart(2, "0")}:${sec}.${milli}`;
  return `${Math.floor(min / 60)}:${String(min % 60).padStart(2, "0")}:${sec}.${milli}`;
}

/** Pretty JSON when `text` is a JSON object or array, else `null`. */
export function prettyJson(text: string): string | null {
  const t = text.trim();
  if (!(t.startsWith("{") || t.startsWith("["))) return null;
  try {
    return JSON.stringify(JSON.parse(t), null, 2);
  } catch {
    return null;
  }
}

/** One line of text for a history row. */
export function summary(event: StreamEvent, decode: (frame: Frame) => string): string {
  switch (event.event) {
    case "message":
      return decode(event.frame).replace(/\s+/g, " ");
    case "opened":
      return `Connected: ${event.head.status} ${event.head.url}`;
    case "closed": {
      const by = event.by_client ? "by client" : "by server";
      return `Closed ${by}${event.code !== null ? ` (${event.code})` : ""}${event.reason ? `: ${event.reason}` : ""}`;
    }
    case "error":
      return `Error (${event.error.kind})${"message" in event.error && event.error.message ? `: ${event.error.message}` : ""}`;
  }
}

/** Events whose text, name or id contains `filter` (case-insensitive); all for an empty one. */
export function filterEvents(events: StreamEvent[], filter: string, decode: (frame: Frame) => string): number[] {
  const f = filter.trim().toLowerCase();
  const out: number[] = [];
  for (let i = 0; i < events.length; i++) {
    const e = events[i];
    if (!f) {
      out.push(i);
      continue;
    }
    const hay =
      e.event === "message" ? `${decode(e.frame)} ${e.name ?? ""} ${e.id ?? ""}` : summary(e, decode);
    if (hay.toLowerCase().includes(f)) out.push(i);
  }
  return out;
}
