// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `InboxApi` of the web app (wave 18, strand E). A browser cannot listen on a port, so the inbox
// runs in the companion (`rt agent`, WebSocket `/v1/inbox`, `InboxClientMessage` /
// `InboxServerMessage` in crates/rt-core/src/transport/companion.rs) and this file mirrors its
// events in memory: `events(since)` and `get` read the mirror, `start`/`stop`/`clear` go over the
// socket. Nothing is stored. The one rule that matters, a capture into a request with its
// secrets as variables without a value, is the core's (`inboxMakeRequest` of rt-wasm).
//
// The socket opens when the inbox view is opened (`status`) or on `start`; without a paired
// companion `status` says "not listening" and `start` explains what to do.

import {
  BackendError,
  type CapturedRequest,
  type InboxApi,
  type InboxDraft,
  type InboxEvent,
  type InboxPage,
  type InboxSettings,
  type InboxStatus,
} from "@requesttailor/ui";

import { toBackendError } from "./wasm";

/** The free functions of rt-wasm for the inbox. */
export interface CoreInbox {
  inboxMakeRequest(request: CapturedRequest): InboxDraft;
}

/** What of a `WebSocket` is used (tests pass a fake). */
export interface SocketLike {
  readonly readyState: number;
  onopen: ((e: unknown) => void) | null;
  onmessage: ((e: { data: unknown }) => void) | null;
  onclose: ((e: unknown) => void) | null;
  onerror: ((e: unknown) => void) | null;
  send(data: string): void;
  close(): void;
}

export interface WebInboxHost {
  /** The companion's URL and token when it is paired, else `null`. */
  credentials(): Promise<{ url: string; token: string } | null>;
  core(): Promise<CoreInbox>;
  webSocket?(url: string): SocketLike;
}

type ServerMessage =
  | { op: "status"; status: InboxStatus }
  | { op: "event"; event: InboxEvent }
  | { op: "failed"; message: string };

const OPEN = 1;
const CONNECT_TIMEOUT_MS = 3000;
const REPLY_TIMEOUT_MS = 5000;
/** After a failed connection `status` waits this long before it tries again. */
const RETRY_MS = 3000;
/** The mirror keeps as much as the core's log. */
const MIRROR_MAX = 1100;

const NOT_PAIRED =
  "The webhook inbox in the browser runs in the companion. Start `rt agent`, pair this page with it (Settings), then press Start.";

const DEFAULT_SETTINGS: InboxSettings = { port: 47855, status: 200, body: "", delay_ms: 0 };
const OFF: InboxStatus = { running: false, held: 0, dropped: 0 };

export function webInbox(host: WebInboxHost): InboxApi {
  let settings: InboxSettings = { ...DEFAULT_SETTINGS };
  let socket: SocketLike | null = null;
  let connecting: Promise<SocketLike> | null = null;
  let failedAt = 0;
  let status: InboxStatus = OFF;
  let seq = 0;
  let mirror: { seq: number; event: InboxEvent }[] = [];
  /** Replies the socket still owes (`status` or `failed`), oldest first. */
  let waiting: { resolve(s: InboxStatus): void; reject(e: Error): void }[] = [];

  const add = (event: InboxEvent) => {
    seq += 1;
    mirror.push({ seq, event });
    if (mirror.length > MIRROR_MAX) mirror = mirror.slice(mirror.length - MIRROR_MAX);
  };

  const closed = (s: SocketLike) => {
    if (socket !== s) return;
    socket = null;
    status = OFF;
    const owed = waiting;
    waiting = [];
    for (const w of owed) w.reject(new BackendError("shell", "The connection to the companion closed."));
  };

  const onMessage = (data: unknown) => {
    let message: ServerMessage;
    try {
      message = JSON.parse(String(data)) as ServerMessage;
    } catch {
      return;
    }
    if (message.op === "event") {
      add(message.event);
    } else if (message.op === "status") {
      status = message.status;
      waiting.shift()?.resolve(message.status);
    } else {
      waiting.shift()?.reject(new BackendError("shell", message.message));
    }
  };

  async function connect(force: boolean): Promise<SocketLike> {
    if (socket && socket.readyState === OPEN) return socket;
    if (connecting) return connecting;
    if (!force && Date.now() - failedAt < RETRY_MS) throw new BackendError("shell", NOT_PAIRED);
    const attempt = (async () => {
      const credentials = await host.credentials();
      if (!credentials) throw new BackendError("shell", NOT_PAIRED);
      const url = new URL(credentials.url);
      const wsUrl = `${url.protocol === "https:" ? "wss:" : "ws:"}//${url.host}/v1/inbox?token=${encodeURIComponent(credentials.token)}`;
      const s = host.webSocket ? host.webSocket(wsUrl) : (new WebSocket(wsUrl) as unknown as SocketLike);
      // The companion replays what it holds: start from an empty list.
      if (mirror.length > 0) add({ kind: "cleared" });
      await new Promise<void>((resolve, reject) => {
        const timer = setTimeout(() => {
          s.close();
          reject(new BackendError("shell", "The companion did not answer. Is `rt agent` running?"));
        }, CONNECT_TIMEOUT_MS);
        s.onopen = () => {
          clearTimeout(timer);
          resolve();
        };
        s.onerror = () => {
          clearTimeout(timer);
          reject(new BackendError("shell", "Could not reach the companion. Is `rt agent` running and is this page paired with it?"));
        };
      });
      s.onmessage = (e) => onMessage(e.data);
      s.onclose = () => closed(s);
      s.onerror = () => closed(s);
      socket = s;
      return s;
    })();
    connecting = attempt;
    try {
      return await attempt;
    } catch (e) {
      failedAt = Date.now();
      throw e;
    } finally {
      connecting = null;
    }
  }

  /** Sends one message and waits for the status (or the failure) it is answered with. */
  async function ask(message: object, force = true): Promise<InboxStatus> {
    const s = await connect(force);
    return new Promise<InboxStatus>((resolve, reject) => {
      const timer = setTimeout(() => reject(new BackendError("shell", "The companion did not answer.")), REPLY_TIMEOUT_MS);
      waiting.push({
        resolve: (st) => {
          clearTimeout(timer);
          resolve(st);
        },
        reject: (e) => {
          clearTimeout(timer);
          reject(e);
        },
      });
      s.send(JSON.stringify(message));
    });
  }

  const find = (id: number): CapturedRequest | null => {
    for (const { event } of mirror) if (event.kind === "request" && event.request.id === id) return event.request;
    return null;
  };

  return {
    settings: async () => ({ ...settings }),
    setSettings: async (next) => {
      settings = { ...next };
      // A running inbox takes the new reply at once (the port waits for the next start).
      if (socket && status.running) await ask({ op: "start", settings });
    },
    start: () => ask({ op: "start", settings }),
    stop: async () => {
      if (!socket) return OFF;
      return ask({ op: "stop" });
    },
    status: async () => {
      try {
        return await ask({ op: "status" }, false);
      } catch {
        return OFF;
      }
    },
    events: async (since): Promise<InboxPage> => ({
      events: mirror.filter((e) => e.seq > since).map((e) => e.event),
      next: seq,
    }),
    get: async (id) => find(id),
    clear: async () => {
      if (socket) await ask({ op: "clear" });
    },
    makeRequest: async (id) => {
      const request = find(id);
      if (!request) throw new BackendError("shell", `The request ${id} is no longer held.`);
      try {
        return (await host.core()).inboxMakeRequest(request);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
