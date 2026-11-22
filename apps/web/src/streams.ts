// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `StreamsApi` of the web app (wave 8, strand A), over rt-wasm's sessions
// (crates/rt-wasm/src/streams_js.rs): `Workspace.openStream` and its `StreamHandle`. A session
// opens like a send: with the secret values, the same services, and through the companion
// when it is on. Without the companion only SSE works (over `fetch`); WebSocket and gRPC
// reject with a message that says the companion is needed.

import {
  BackendError,
  type MessageVariant,
  type SendInput,
  type StreamEvent,
  type StreamLog,
  type StreamsApi,
} from "@requesttailor/ui";

import type { CoreSendServices } from "./send-services";
import type { RunContext } from "./runner";
import { toBackendError, type CompanionCredentials, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** rt-wasm's `StreamHandle`. */
export interface CoreStreamHandle {
  send(message: MessageVariant): void;
  finish(): void;
  close(): void;
  log(): StreamLog;
  readonly ended: boolean;
  release(): void;
  free?(): void;
}

/** The sessions' part of rt-wasm's `Workspace`. */
export interface CoreStreamsWorkspace {
  openStream(
    path: string,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    onEvent: (event: StreamEvent) => void,
  ): Promise<CoreStreamHandle>;
}

export interface WebStreamsHost {
  session(): Session;
  /** The input with secret values, the companion (or `null`) and the services of a send. */
  sendContext(input: SendInput): Promise<RunContext>;
}

async function guard<T>(f: () => Promise<T> | T): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

export function webStreams(host: WebStreamsHost): StreamsApi {
  const handles = new Map<number, CoreStreamHandle>();
  let next = 0;
  const handle = (session: number): CoreStreamHandle => {
    const h = handles.get(session);
    if (!h) throw new BackendError("shell", `no session ${session}`);
    return h;
  };
  return {
    open: (path, input, onEvent) =>
      guard(async () => {
        const ws = host.session().ws as unknown as CoreStreamsWorkspace;
        const context = await host.sendContext(input);
        const h = await ws.openStream(path, context.input, context.companion, context.services, onEvent);
        next += 1;
        handles.set(next, h);
        return next;
      }),
    send: (session, message) => guard(() => handle(session).send(message)),
    finish: (session) => guard(() => handle(session).finish()),
    close: (session) => guard(() => handle(session).close()),
    log: (session) => guard(() => handle(session).log()),
    release: (session) =>
      guard(() => {
        const h = handles.get(session);
        if (!h) return;
        handles.delete(session);
        h.release();
        h.free?.();
      }),
  };
}
