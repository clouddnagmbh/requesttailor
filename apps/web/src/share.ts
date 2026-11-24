// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShareApi` of the web app (wave 17, strand C), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/share_js.rs). Interface placeholder: every method rejects with kind
// `unsupported` until strand C fills this file in.

import { stubShare, type ShareApi } from "@requesttailor/ui";

import type { Session } from "./web-backend";

export function webShare(session: () => Session): ShareApi {
  void session;
  return stubShare();
}
