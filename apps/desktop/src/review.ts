// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the desktop app (wave 12, strand E; src-tauri/src/review.rs): pull and merge
// requests of the collection's Git host. The token lives in the keychain on the Rust side;
// the webview can set and delete it and ask whether one exists, never read it back.

import type { ReviewApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

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
  };
}

/** Commands the shared mapping test in backend.test.ts leaves out: its call list is one
 *  sequence every strand would edit. `review.test.ts` checks all of them instead. */
export const PENDING: readonly Command[] = [
  "review_hosts",
  "review_set_hosts",
  "review_repo",
  "review_has_token",
  "review_set_token",
  "review_delete_token",
  "review_start_login",
  "review_finish_login",
  "review_list",
  "review_open",
  "review_comments",
  "review_comment",
  "review_submit",
  "review_ci",
];
