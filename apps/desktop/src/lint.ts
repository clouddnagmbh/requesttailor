// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand F: lint, one Tauri command per method (`src-tauri/src/lint.rs`,
// placeholders until strand F).

import type { LintApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriLint(call: Call): LintApi {
  return {
    run: () => call("lint_run"),
    settings: () => call("lint_settings"),
    setSettings: (settings) => call("lint_set_settings", { settings }),
    ignore: (path, rule) => call("lint_ignore", { path, rule }),
  };
}
