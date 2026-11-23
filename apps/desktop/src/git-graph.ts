// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: the commit graph of the desktop app, one Tauri command per method
// (`src-tauri/src/git_graph.rs`, placeholders until strand F).

import type { GitGraphMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitGraph(call: Call): GitGraphMethods {
  return {
    graph: (query) => call("git_graph", { query }),
    refs: () => call("git_refs"),
    changes: (from, to) => call("git_changes", { from, to }),
    fileDiff: (from, to, path) => call("git_file_diff", { from, to, path }),
  };
}
