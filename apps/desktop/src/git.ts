// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GitApi` of the desktop app: one Tauri command per method (src-tauri/src/git.rs).
// Owner: wave 4, strand A1.

import type { GitApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGit(call: Call): GitApi {
  return {
    status: () => call("git_status"),
    diff: (path) => call("git_diff", { path }),
    commit: (message, paths) => call("git_commit", { message, paths }),
    log: (limit) => call("git_log", { limit }),
    branches: () => call("git_branches"),
    checkout: (branch) => call("git_checkout", { branch }),
    createBranch: (name, checkout) => call("git_create_branch", { name, checkout }),
    push: () => call("git_push"),
    pull: () => call("git_pull"),
    conflict: (path) => call("git_conflict", { path }),
    resolve: (path, resolution) => call("git_resolve", { path, resolution }),
    init: () => call("git_init"),
    clone: (url) => call("git_clone", { url }),
    identity: () => call("git_identity"),
    setIdentity: (identity) => call("git_set_identity", { identity }),
    hasCredentials: (host) => call("git_has_credentials", { host }),
    setCredentials: (host, username, password) => call("git_set_credentials", { host, username, password }),
    deleteCredentials: (host) => call("git_delete_credentials", { host }),
  };
}
