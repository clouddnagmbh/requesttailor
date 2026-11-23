// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand F: the index and signatures (`src-tauri/src/git_index.rs`, placeholders
// until strand F).

import type { GitIndexMethods } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGitIndex(call: Call): GitIndexMethods {
  return {
    indexDiff: (path, side) => call("git_index_diff", { path, side }),
    stage: (paths) => call("git_stage", { paths }),
    unstage: (paths) => call("git_unstage", { paths }),
    stagePatch: (patch) => call("git_stage_patch", { patch }),
    unstagePatch: (patch) => call("git_unstage_patch", { patch }),
    discardPatch: (patch) => call("git_discard_patch", { patch }),
    commitStaged: (commit) => call("git_commit_staged", { commit }),
    verify: (oids) => call("git_verify", { oids }),
  };
}
