// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DebuggerApi` of the web app (wave 19, strand Y), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/debugger_js.rs): the same engine as the desktop app, a paused script is a
// suspended call kept in the page. A send's input (environment, session variables, secret
// values from the encrypted browser store) goes in so the script sees what a send gives it; the
// debugger only ever shows masked values. Scripts run only in a trusted collection.

import type { DebugCommand, DebugState, DebugTarget, DebuggerApi, SendInput } from "@requesttailor/ui";
import { BackendError } from "@requesttailor/ui";

import { toBackendError, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** The debugger part of rt-wasm's `Workspace`. */
export interface CoreDebuggerWorkspace {
  debuggerStart(target: DebugTarget, input: CoreSendInput): Promise<DebugState>;
  debuggerCommand(session: number, command: DebugCommand): Promise<DebugState>;
  debuggerState(session: number): DebugState;
  debuggerStop(session: number): void;
}

export interface DebuggerDeps {
  session: () => Session;
  /** Whether the user trusts the collection to run scripts. */
  trusted: (collectionId: string) => boolean;
  /** The input of a send for `environment` (session variables, secret values). */
  input: (s: Session, input: SendInput) => Promise<CoreSendInput>;
}

export function webDebugger(deps: DebuggerDeps): DebuggerApi {
  const run = async <T>(f: (ws: CoreDebuggerWorkspace, s: Session) => T | Promise<T>): Promise<T> => {
    try {
      const s = deps.session();
      return await f(s.ws as unknown as CoreDebuggerWorkspace, s);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    start: (target) =>
      run(async (ws, s) => {
        if (!deps.trusted(s.id)) {
          throw new BackendError("shell", "Trust this collection to run its scripts before debugging them.");
        }
        const input = await deps.input(s, { environment: target.environment ?? null, vars: [], secrets: [] });
        return ws.debuggerStart(target, input);
      }),
    command: (session, command) => run((ws) => ws.debuggerCommand(session, command)),
    state: (session) => run((ws) => ws.debuggerState(session)),
    stop: (session) => run((ws) => ws.debuggerStop(session)),
  };
}
