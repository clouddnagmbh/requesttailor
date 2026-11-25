// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InboxApi` of the mock backend (wave 18, strand E), for UI tests (`?backend=mock`). Nothing
// listens: the page hands requests to `window.__rtMockInbox.push(...)` as if they had arrived.
// The buffer limits (500 requests) and `makeRequest` mirror `rt_app::inbox` in a simplified way
// (secret names become variables without a value); the core's rules are tested in Rust.

import type { InboxApi } from "./backend";
import { BackendError } from "./backend";
import type { CapturedRequest, InboxDraft, InboxEvent, InboxSettings, InboxStatus, KeyValue } from "./types";

const MAX_ENTRIES = 500;
const DROPPED_HEADERS = ["host", "connection", "keep-alive", "te", "trailer", "transfer-encoding", "upgrade", "content-length", "expect", "accept-encoding"];
const SENSITIVE = ["signature", "token", "secret", "key", "password"];
const SENSITIVE_NAMES = ["authorization", "proxy-authorization", "cookie"];

const sensitive = (name: string) => {
  const n = name.toLowerCase();
  return SENSITIVE_NAMES.includes(n) || SENSITIVE.some((p) => n.includes(p));
};
const slug = (name: string) =>
  name
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "") || "secret";

export type PushedRequest = Partial<CapturedRequest> & Pick<CapturedRequest, "method" | "target">;

export interface MockInboxHook {
  push(request: PushedRequest): number;
}

declare global {
  interface Window {
    __rtMockInbox?: MockInboxHook;
  }
}

export function mockInbox(): InboxApi {
  let settings: InboxSettings = { port: 47855, status: 200, body: "", delay_ms: 0 };
  let running = false;
  let port: number | undefined;
  let nextId = 1;
  let seq = 0;
  let dropped = 0;
  let log: { seq: number; event: InboxEvent }[] = [];

  const add = (event: InboxEvent) => {
    seq += 1;
    log.push({ seq, event });
  };
  const held = () => log.filter((e) => e.event.kind === "request").length;
  const status = (): InboxStatus => ({
    running,
    held: held(),
    dropped,
    ...(running && port !== undefined ? { port, url: `http://127.0.0.1:${port}/` } : {}),
  });

  const push = (r: PushedRequest): number => {
    if (!running) throw new Error("The mock inbox is not started.");
    const request: CapturedRequest = {
      id: nextId++,
      at: Date.now(),
      headers: [],
      body: null,
      size: r.body?.length ?? 0,
      ...r,
    };
    add({ kind: "request", request });
    const requests = log.filter((e) => e.event.kind === "request");
    if (requests.length > MAX_ENTRIES) {
      const old = requests[0];
      log = log.filter((e) => e !== old);
      dropped += 1;
      add({ kind: "dropped", count: 1 });
    }
    return request.id;
  };
  if (typeof window !== "undefined") window.__rtMockInbox = { push };

  const find = (id: number): CapturedRequest | null => {
    for (const { event } of log) if (event.kind === "request" && event.request.id === id) return event.request;
    return null;
  };

  return {
    settings: async () => ({ ...settings }),
    setSettings: async (s) => {
      if (s.status < 200 || s.status > 599) throw new BackendError("shell", "The status must be between 200 and 599.");
      if (s.delay_ms > 5000) throw new BackendError("shell", "The delay is at most 5 seconds.");
      settings = { ...s };
    },
    start: async () => {
      if (!running) {
        running = true;
        port = settings.port === 0 ? 49152 : settings.port;
        add({ kind: "started", port });
      }
      return status();
    },
    stop: async () => {
      if (running) {
        running = false;
        add({ kind: "stopped" });
        port = undefined;
      }
      return status();
    },
    status: async () => status(),
    events: async (since) => ({ events: log.filter((e) => e.seq > since).map((e) => e.event), next: seq }),
    get: async (id) => find(id),
    clear: async () => {
      log = log.filter((e) => e.event.kind !== "request" && e.event.kind !== "dropped");
      add({ kind: "cleared" });
    },
    makeRequest: async (id): Promise<InboxDraft> => {
      const r = find(id);
      if (!r) throw new BackendError("shell", `The request ${id} is no longer held.`);
      const secrets: string[] = [];
      const variable = (name: string) => {
        const s = slug(name);
        if (!secrets.includes(s)) secrets.push(s);
        return `{{${s}}}`;
      };
      const [path, query] = r.target.split("?", 2);
      const pairs = (query ?? "")
        .split("&")
        .filter((p) => p !== "")
        .map((p) => {
          const [n, v = ""] = p.split("=", 2);
          return [n, sensitive(n) && v !== "" ? variable(n) : v] as const;
        });
      const headers: KeyValue[] = r.headers
        .filter(([n]) => !DROPPED_HEADERS.includes(n.toLowerCase()))
        .map(([name, value]) => ({ name, value: sensitive(name) ? variable(name) : value, enabled: true }));
      const name = `${r.method} ${path}`;
      const contentType = r.headers.find(([n]) => n.toLowerCase() === "content-type")?.[1] ?? "";
      return {
        name,
        secrets,
        spec: {
          name,
          seq: null,
          method: r.method,
          url: `{{baseUrl}}${path}${pairs.length > 0 ? `?${pairs.map(([n, v]) => `${n}=${v}`).join("&")}` : ""}`,
          headers,
          params: pairs.map(([n, v]) => ({ name: n, value: v, kind: "query", enabled: true })),
          body: r.body ? { type: "raw", kind: contentType.includes("json") ? "json" : "text", data: r.body } : { type: "none" },
          auth: { type: "inherit" },
          vars: [],
          settings: {},
          scripts: [],
          assertions: [],
          actions: [],
        } as unknown as InboxDraft["spec"],
      };
    },
  };
}
