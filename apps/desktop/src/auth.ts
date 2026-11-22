// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AuthApi` of the desktop app (src-tauri/src/auth.rs). Prompts arrive as the Tauri event
// `auth-prompt` (the authorization URL the Rust side opened in the system browser, a device
// code, or `done`); they never carry a token. Owner: wave 5, strand C.

import { listen as tauriListen } from "@tauri-apps/api/event";
import type { AuthApi, AuthPrompt } from "@requesttailor/ui";

import type { Call } from "./backend";

export const PROMPT_EVENT = "auth-prompt";

/** `listen` from `@tauri-apps/api/event`, injectable for tests. */
export type Listen = (
  event: string,
  handler: (event: { payload: AuthPrompt }) => void,
) => Promise<() => void>;

export function tauriAuth(call: Call, listen: Listen = tauriListen as Listen): AuthApi {
  return {
    tokens: () => call("auth_tokens"),
    fetchToken: (path, input) =>
      // Secret values come from the keychain on the Rust side, never from the webview.
      call("auth_fetch_token", {
        path,
        input: { environment: input.environment, vars: input.vars },
      }),
    clearTokens: (key) => call("auth_clear_tokens", { key }),
    onPrompt: (handler) => {
      // `listen` resolves asynchronously; an unsubscribe before that still takes effect.
      let unlisten: (() => void) | null = null;
      let stopped = false;
      listen(PROMPT_EVENT, (event) => handler(event.payload)).then(
        (stop) => {
          if (stopped) stop();
          else unlisten = stop;
        },
        (e: unknown) => console.error("RequestTailor: cannot listen to OAuth 2.0 prompts", e),
      );
      return () => {
        stopped = true;
        unlisten?.();
        unlisten = null;
      };
    },
    cancelPrompt: () => call("auth_cancel_prompt"),
  };
}
