// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `SnapshotApi` of the web app (wave 17, strand B), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/snapshots_js.rs). Interface placeholder: every method rejects with kind
// `unsupported` until strand B fills this file in.

import { stubSnapshots, type SnapshotApi } from "@requesttailor/ui";

import type { Session } from "./web-backend";

export function webSnapshots(session: () => Session): SnapshotApi {
  void session;
  return stubSnapshots();
}
