// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I: the plugin catalogue, one Tauri command per method (`src-tauri/src/catalogue.rs`,
// placeholders until strand I).

import type { CatalogueApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriCatalogue(call: Call): CatalogueApi {
  return {
    sources: () => call("catalogue_sources"),
    addSource: (url) => call("catalogue_add_source", { url }),
    removeSource: (url) => call("catalogue_remove_source", { url }),
    refresh: () => call("catalogue_refresh"),
    plan: (source, id) => call("catalogue_plan", { source, id }),
    install: (plan) => call("catalogue_install", { plan }),
    installed: () => call("catalogue_installed"),
    checkUpdates: () => call("catalogue_check_updates"),
  };
}
