// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the Git wizards' backend of the desktop app, one Tauri command per method
// (`src-tauri/src/git_actions.rs`, placeholders until strand G).

import type { GitActionMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitActions(call: Call): GitActionMethods {
  return {
    remotes: () => call("git_remotes"),
    stashes: () => call("git_stashes"),
    plan: (action) => call("git_plan", { action }),
    apply: (action) => call("git_apply", { action }),
    lastAction: () => call("git_last_action"),
    undo: (id) => call("git_undo", { id }),
  };
}
