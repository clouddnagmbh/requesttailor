// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `FolderScriptsApi` of the desktop app (src-tauri/src/folder_scripts.rs, wave 16).

import type { FolderScriptsApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriFolderScripts(call: Call): FolderScriptsApi {
  return {
    get: (path) => call("folder_scripts_get", { path }),
    set: (path, scripts) => call("folder_scripts_set", { path, scripts }),
  };
}
