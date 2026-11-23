// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `FolderScriptsApi` of the web app (wave 16), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/folder_scripts_js.rs).

import type { FolderScriptsApi, Script } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The folder-scripts part of rt-wasm's `Workspace`. */
export interface CoreFolderScriptsWorkspace {
  folderScripts(folder: string): Script[];
  setFolderScripts(folder: string, scripts: Script[]): void;
}

export function webFolderScripts(session: () => Session): FolderScriptsApi {
  const ws = () => session().ws as unknown as CoreFolderScriptsWorkspace;
  return {
    get: async (path) => {
      try {
        return ws().folderScripts(path);
      } catch (e) {
        throw toBackendError(e);
      }
    },
    set: async (path, scripts) => {
      try {
        ws().setFolderScripts(path, scripts);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
