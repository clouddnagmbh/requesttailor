// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D: commit building blocks and the text conflict result
// (`src-tauri/src/git_plumbing.rs`, placeholders until strand D).

import type { GitPlumbingMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitPlumbing(call: Call): GitPlumbingMethods {
  return {
    commitInfo: (rev) => call("git_commit_info", { rev }),
    pick: (request) => call("git_pick", { request }),
    writeTree: (write) => call("git_write_tree", { write }),
    writeCommit: (write) => call("git_write_commit", { write }),
    resolveText: (path, text) => call("git_resolve_text", { path, text }),
  };
}
