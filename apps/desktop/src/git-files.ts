// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: working copy, file tree and file contents of the active repository
// (`src-tauri/src/git_files.rs`, placeholders until strand B).

import type { GitWorkingMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitWorking(call: Call): GitWorkingMethods {
  return {
    workingCopy: () => call("git_working_copy"),
    fileTree: (rev, dir) => call("git_file_tree", { rev, dir }),
    blob: (rev, path) => call("git_blob", { rev, path }),
  };
}
