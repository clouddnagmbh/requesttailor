// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Browser side of the companion (`rt agent`): discovery, pairing, token storage.
// Protocol: crates/rt-core/src/transport/companion.rs. Owner: wave 3, strand G.
// The WebBackend (strand E) uses this for `Backend.transport` and passes `url` and `token`
// to rt-wasm, which sends through `CompanionTransport`.

import type { CompanionStatus } from "@requesttailor/ui";

export const DEFAULT_COMPANION_URL = "http://127.0.0.1:47812";

export interface CompanionClient {
  status(): Promise<CompanionStatus>;
  /** Pairs this origin with the running companion; resolves when a token is stored. */
  pair(): Promise<void>;
  /** Base URL and token for rt-wasm, or `null` when not paired. */
  credentials(): Promise<{ url: string; token: string } | null>;
  forget(): Promise<void>;
}

export function createCompanionClient(url: string = DEFAULT_COMPANION_URL): CompanionClient {
  void url;
  throw new Error("createCompanionClient: wave 3, strand G");
}
