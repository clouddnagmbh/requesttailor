// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: saved views, one Tauri command per method (`src-tauri/src/views.rs`,
// placeholders until strand G).

import type { ViewsApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriViews(call: Call): ViewsApi {
  return {
    list: (path) => call("views_list", { path }),
    save: (path, view) => call("views_save", { path, view }),
    remove: (path, name) => call("views_remove", { path, name }),
    apply: (path, responseId, view) => call("views_apply", { path, responseId, view }),
    languages: () => call("views_languages"),
  };
}
