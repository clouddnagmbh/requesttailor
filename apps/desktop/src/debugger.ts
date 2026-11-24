// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: the script debugger, one Tauri command per method (`src-tauri/src/debugger.rs`,
// placeholders until strand Y).

import type { DebuggerApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriDebugger(call: Call): DebuggerApi {
  return {
    start: (target) => call("debugger_start", { target }),
    command: (session, command) => call("debugger_command", { session, command }),
    state: (session) => call("debugger_state", { session }),
    stop: (session) => call("debugger_stop", { session }),
  };
}
