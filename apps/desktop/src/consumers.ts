// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand F: consumer contracts, one Tauri command per method (`src-tauri/src/consumers.rs`,
// placeholders until strand F).

import type { ConsumersApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriConsumers(call: Call): ConsumersApi {
  return {
    list: () => call("consumers_list"),
    verify: (request) => call("consumers_verify", { request }),
  };
}
