// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand D: the access check, one Tauri command per method (`src-tauri/src/access.rs`,
// placeholders until strand D).

import type { AccessApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriAccess(call: Call): AccessApi {
  return {
    check: (path, profile, options) => call("access_check", { path, profile, options }),
  };
}
