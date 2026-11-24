// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: the one-time question "Allow <extension> to contact <host> for <purpose>?"
// (a dialog over `AppState.showDialog`). Interface placeholder: never allows, so an extension
// that needs a host gets the `not-allowed` error and nothing is sent.

import type { AppState } from "../state/app.svelte";
import type { EgressRequest } from "../commands/types";

export async function askEgress(app: AppState, request: EgressRequest): Promise<boolean> {
  void app;
  void request;
  return false;
}
