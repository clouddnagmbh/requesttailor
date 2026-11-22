// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// OAuth 2.0 tokens and prompts in the mock backend (wave 5, strand C owns this file).
// Interface commit: no tokens, no flows.

import { BackendError, type AuthApi } from "./backend";
import type { AuthPrompt } from "./types";

export function mockAuth(): AuthApi {
  const handlers = new Set<(prompt: AuthPrompt) => void>();
  return {
    tokens: async () => [],
    fetchToken: async () => {
      throw new BackendError("auth", "OAuth 2.0 is not available in the mock yet");
    },
    clearTokens: async () => {},
    onPrompt: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    cancelPrompt: async () => {
      for (const h of handlers) h({ kind: "done" });
    },
  };
}
