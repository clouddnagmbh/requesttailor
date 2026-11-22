// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AuthApi` of the web app (wave 5, strand C): tokens in the encrypted browser store, the
// authorization code flow in a popup that returns to the app's origin, device code shown in
// the UI. Token requests go through the same transport as sends (browser or companion).
// Interface commit: no tokens, no flows.

import { BackendError, type AuthApi, type AuthPrompt } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

/** `AuthApi` plus the auth part of a send (the `auth` field `services_js` reads). */
export interface WebAuthApi extends AuthApi {
  forSend(collectionId: string, environment: string | null): Promise<unknown>;
}

export function webAuth(_host: WebHost): WebAuthApi {
  const handlers = new Set<(prompt: AuthPrompt) => void>();
  return {
    tokens: async () => [],
    fetchToken: async () => {
      throw new BackendError("unsupported", "OAuth 2.0 is not available yet");
    },
    clearTokens: async () => {},
    onPrompt: (handler) => {
      handlers.add(handler);
      return () => handlers.delete(handler);
    },
    cancelPrompt: async () => {},
    forSend: async () => undefined,
  };
}
