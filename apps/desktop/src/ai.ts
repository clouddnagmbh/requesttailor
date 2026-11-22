// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AiApi` of the desktop app (src-tauri/src/ai.rs). Owner: wave 8, strand E. The config lives
// in the data folder, keys in the OS keychain; the webview never reads a key back.

import type { AiApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriAi(call: Call): AiApi {
  return {
    config: () => call("ai_config"),
    setConfig: (config) => call("ai_set_config", { config }),
    hasKey: (provider) => call("ai_has_key", { provider }),
    setKey: (provider, key) => call("ai_set_key", { provider, key }),
    deleteKey: (provider) => call("ai_delete_key", { provider }),
    // Secret values are never sent from the webview; the Rust side reads the keychain.
    ask: (task, input) => call("ai_ask", { task, input: { environment: input.environment, vars: input.vars } }),
  };
}
