// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B: egress for extensions, one Tauri command per method (`src-tauri/src/egress.rs`,
// placeholders until strand B).

import type { EgressApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriEgress(call: Call): EgressApi {
  return {
    hosts: () => call("egress_hosts"),
    allow: (host, port, extension, purpose) => call("egress_allow", { host, port, extension, purpose }),
    remove: (host, port) => call("egress_remove", { host, port }),
    request: (request) => call("egress_request", { request }),
  };
}
