// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Open sessions of the shell, one per request path (wave 8, strand A). A session stays open
// while its tab is open (switching tabs keeps it); closing the tab or opening another
// collection closes and releases it. Events arrive through `backend.streams.open`'s callback
// and are batched per animation frame, so a fast stream does not re-render per message.

import { getContext, setContext } from "svelte";

import type { StreamEvent } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import { messagesOf, selectedMessage, sessionKind, type SessionKind } from "./kind";

const KEY = Symbol("requesttailor-sessions");

export type SessionStatus = "connecting" | "open" | "closed" | "error";

export class SessionEntry {
  readonly path: string;
  readonly kind: SessionKind;
  id = $state<number | null>(null);
  status = $state<SessionStatus>("connecting");
  /** Replaced (never mutated) when events arrive, so views can take it as a prop. */
  events = $state.raw<StreamEvent[]>([]);
  /** Opening or sending failed (backend error, not a session event). */
  error = $state<string | null>(null);
  /** Number of messages handed to `send` (for gRPC unary: at most one). */
  sent = $state(0);
  private buffer: StreamEvent[] = [];
  private scheduled = false;

  constructor(path: string, kind: SessionKind) {
    this.path = path;
    this.kind = kind;
  }

  push(event: StreamEvent) {
    this.buffer.push(event);
    if (event.event === "opened") this.status = "open";
    else if (event.event === "closed") this.status = "closed";
    else if (event.event === "error") this.status = "error";
    if (this.scheduled) return;
    this.scheduled = true;
    const run = () => this.flush();
    if (typeof requestAnimationFrame === "function") requestAnimationFrame(run);
    else queueMicrotask(run);
  }

  flush() {
    this.scheduled = false;
    if (this.buffer.length === 0) return;
    this.events = [...this.events, ...this.buffer];
    this.buffer = [];
  }

  get live(): boolean {
    return this.status === "connecting" || this.status === "open";
  }
}

export class SessionsState {
  readonly app: AppState;
  /** Latest session per request path (also after it closed, to show its history). */
  entries = $state<Record<string, SessionEntry>>({});

  constructor(app: AppState) {
    this.app = app;
  }

  kindOf(path: string | null): SessionKind | null {
    return path ? sessionKind(this.app.drafts[path]) : null;
  }

  /** Connects, or (when open) sends the selected message: what Send and Mod+Enter do. */
  async primary(path: string): Promise<void> {
    const entry = this.entries[path];
    if (entry?.live) {
      if (entry.kind !== "sse" && entry.status === "open") await this.sendSelected(path);
      return;
    }
    await this.connect(path);
  }

  async toggle(path: string): Promise<void> {
    if (this.entries[path]?.live) await this.disconnect(path);
    else await this.connect(path);
  }

  async connect(path: string): Promise<void> {
    const kind = this.kindOf(path);
    if (!kind) return;
    await this.dispose(path);
    const entry = new SessionEntry(path, kind);
    this.entries[path] = entry;
    const streams = this.app.backend.streams;
    try {
      await this.app.settled();
      const id = await streams.open(path, { environment: this.app.environment, vars: [] }, (e) => entry.push(e));
      // Closed or replaced while opening: nobody shows this session any more.
      if (this.entries[path] !== entry) {
        await streams.close(id).catch(() => {});
        await streams.release(id).catch(() => {});
        return;
      }
      entry.id = id;
    } catch (e) {
      entry.flush();
      entry.status = "error";
      entry.error = message(e);
    }
  }

  async sendSelected(path: string): Promise<void> {
    const entry = this.entries[path];
    const message = selectedMessage(messagesOf(this.app.drafts[path]));
    if (!entry || entry.id === null || !message) return;
    try {
      await this.app.settled();
      await this.app.backend.streams.send(entry.id, $state.snapshot(message));
      entry.sent++;
    } catch (e) {
      entry.error = errorText(e);
    }
  }

  /** gRPC client and bidi streaming: no more messages. */
  async finish(path: string): Promise<void> {
    const entry = this.entries[path];
    if (!entry || entry.id === null) return;
    try {
      await this.app.backend.streams.finish(entry.id);
    } catch (e) {
      entry.error = errorText(e);
    }
  }

  async disconnect(path: string): Promise<void> {
    const entry = this.entries[path];
    if (!entry || entry.id === null || !entry.live) return;
    try {
      await this.app.backend.streams.close(entry.id);
    } catch (e) {
      entry.error = errorText(e);
    }
  }

  /** Closes and releases the session of `path` and forgets its history. */
  async dispose(path: string): Promise<void> {
    const entry = this.entries[path];
    if (!entry) return;
    delete this.entries[path];
    const id = entry.id;
    if (id === null) return;
    const streams = this.app.backend.streams;
    if (entry.live) await streams.close(id).catch(() => {});
    await streams.release(id).catch(() => {});
  }

  /** Disposes the sessions of requests that are no longer open in a tab. */
  async keepOnly(paths: string[]): Promise<void> {
    for (const path of Object.keys(this.entries)) if (!paths.includes(path)) await this.dispose(path);
  }
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
const errorText = message;

export function provideSessions(state: SessionsState): SessionsState {
  return setContext(KEY, state);
}

/** The shell's sessions; `undefined` outside `<AppShell>`. */
export function getSessions(): SessionsState | undefined {
  return getContext<SessionsState | undefined>(KEY);
}
