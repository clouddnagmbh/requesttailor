// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// OAuth 2.0 tokens and prompts in the mock backend (wave 5, strand C). `fetchToken` plays a
// flow: a browser prompt (or a device code for paths containing "device"), then the token
// after `delayMs`, unless `cancelPrompt` comes first. Tokens live in memory, without values.

import { BackendError, type AuthApi } from "./backend";
import type { AuthPrompt, TokenInfo } from "./types";

export function mockAuth(delayMs = 400): AuthApi {
  const handlers = new Set<(prompt: AuthPrompt) => void>();
  const tokens = new Map<string, TokenInfo>();
  let waiting: { cancel: () => void } | null = null;
  const emit = (prompt: AuthPrompt) => {
    for (const h of handlers) h(prompt);
  };

  return {
    tokens: async () => [...tokens.values()],
    fetchToken: async (path) => {
      waiting?.cancel();
      const state = encodeURIComponent(path);
      emit(
        path.includes("device")
          ? {
              kind: "device-code",
              user_code: "WDJB-MJHT",
              verification_uri: "https://idp.example/device",
              verification_uri_complete: "https://idp.example/device?user_code=WDJB-MJHT",
              expires_in: 600,
            }
          : {
              kind: "browser",
              url: `https://idp.example/authorize?response_type=code&client_id=mock-client&state=${state}`,
            },
      );
      try {
        await new Promise<void>((resolve, reject) => {
          const timer = setTimeout(resolve, delayMs);
          waiting = {
            cancel: () => {
              clearTimeout(timer);
              reject(new BackendError("auth", "cancelled"));
            },
          };
        });
      } finally {
        waiting = null;
        emit({ kind: "done" });
      }
      const now = Date.now();
      const info: TokenInfo = {
        key: `mock|${path}`,
        label: "idp.example · mock-client",
        obtained_at: now,
        expires_at: now + 3_600_000,
        refreshable: true,
      };
      tokens.set(info.key, info);
      return info;
    },
    clearTokens: async (key) => {
      if (key == null) tokens.clear();
      else tokens.delete(key);
    },
    onPrompt: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    cancelPrompt: async () => {
      if (waiting) waiting.cancel();
      else emit({ kind: "done" });
    },
  };
}
