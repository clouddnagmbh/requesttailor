// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand R: passive security checks, one Tauri command per method (`src-tauri/src/security.rs`,
// placeholders until strand R).

import type { SecurityApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriSecurity(call: Call): SecurityApi {
  return {
    check: (target) => call("security_check", { target }),
    rules: () => call("security_rules"),
    settings: () => call("security_settings"),
    setSettings: (settings) => call("security_set_settings", { settings }),
  };
}
