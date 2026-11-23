// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: blame, history search and reflog (`src-tauri/src/git_history.rs`,
// placeholders until strand C).

import type { GitHistoryMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitHistory(call: Call): GitHistoryMethods {
  return {
    blame: (rev, path) => call("git_blame", { rev, path }),
    searchHistory: (query) => call("git_search_history", { query }),
    reflog: (name, limit) => call("git_reflog", { name, limit }),
  };
}
