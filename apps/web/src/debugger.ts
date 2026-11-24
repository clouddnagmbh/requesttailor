// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DebuggerApi` of the web app (wave 19, strand Y): the placeholder of `@requesttailor/ui` until the
// strand answers it over rt-wasm (`crates/rt-wasm/src/debugger_js.rs`).

import { stubDebugger, type DebuggerApi } from "@requesttailor/ui";

export function webDebugger(): DebuggerApi {
  return stubDebugger();
}
