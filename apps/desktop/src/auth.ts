// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AuthApi` of the desktop app (src-tauri/src/auth.rs). Prompts arrive as the Tauri event
// `auth-prompt`. Owner: wave 5, strand C. Interface commit: no prompts are delivered yet.

import type { AuthApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriAuth(call: Call): AuthApi {
  return {
    tokens: () => call("auth_tokens"),
    fetchToken: (path, input) => call("auth_fetch_token", { path, input }),
    clearTokens: (key) => call("auth_clear_tokens", { key }),
    onPrompt: () => () => {},
    cancelPrompt: () => call("auth_cancel_prompt"),
  };
}
