// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CookieApi` of the desktop app (src-tauri/src/cookies.rs). Owner: wave 5, strand D.

import type { CookieApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriCookies(call: Call): CookieApi {
  return {
    list: (environment) => call("cookie_list", { environment }),
    delete: (environment, domain, path, name) => call("cookie_delete", { environment, domain, path, name }),
    clear: (environment) => call("cookie_clear", { environment }),
  };
}
