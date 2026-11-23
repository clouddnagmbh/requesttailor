// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the desktop app (wave 12, strand E; src-tauri/src/review.rs): pull and merge
// requests of the collection's Git host. The token lives in the keychain on the Rust side;
// the webview can set and delete it and ask whether one exists, never read it back.

import type { ReviewApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriReview(call: Call): ReviewApi {
  return {
    repo: () => call("review_repo"),
    hosts: () => call("review_hosts"),
    setHosts: (hosts) => call("review_set_hosts", { hosts }),
    hasToken: (host) => call("review_has_token", { host }),
    setToken: (host, token) => call("review_set_token", { host, token }),
    deleteToken: (host) => call("review_delete_token", { host }),
    startLogin: (host) => call("review_start_login", { host }),
    finishLogin: (host) => call("review_finish_login", { host }),
    list: () => call("review_list"),
    open: (number) => call("review_open", { number }),
    comments: (number) => call("review_comments", { number }),
    comment: (number, comment) => call("review_comment", { number, comment }),
    submit: (number, review) => call("review_submit", { number, review }),
    ci: (number) => call("review_ci", { number }),
    create: (pull) => call("review_create", { pull }),
  };
}
