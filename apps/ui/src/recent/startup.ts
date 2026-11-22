// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the app does at start before the user does anything: reopen the last collection
// without a prompt where the platform allows it (budget: send a request in under three
// keystrokes from app start). Owner: wave 4, strand C.

import type { CollectionInfo } from "../commands/backend";
import type { AppState } from "../state/app.svelte";

/** The collection to show at start, opened by the backend, or `null` for the start screen. */
export async function reopenOnStart(app: AppState): Promise<CollectionInfo | null> {
  void app;
  return null;
}
