// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: response checks, one Tauri command per method (`src-tauri/src/inspect.rs`,
// placeholders until strand B).

import type { InspectApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriInspect(call: Call): InspectApi {
  return {
    jwt: (token) => call("inspect_jwt", { token }),
    tokens: (target) => call("inspect_tokens", { target }),
    checks: (target) => call("inspect_checks", { target }),
    ignoreCheck: (id, scope) => call("inspect_ignore_check", { id, scope }),
  };
}
