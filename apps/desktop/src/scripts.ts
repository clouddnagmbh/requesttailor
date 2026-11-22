// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ScriptsApi` of the desktop app: trust (src-tauri/src/scripts.rs, wave 7 strand A) and
// session variables (src-tauri/src/session_vars.rs, strand C).

import type { ScriptsApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriScripts(call: Call): ScriptsApi {
  return {
    trust: () => call("scripts_trust"),
    setTrusted: (trusted) => call("scripts_set_trusted", { trusted }),
    sessionVars: (environment) => call("scripts_session_vars", { environment }),
    setSessionVar: (environment, change) => call("scripts_set_session_var", { environment, change }),
    clearSessionVars: (environment) => call("scripts_clear_session_vars", { environment }),
  };
}
