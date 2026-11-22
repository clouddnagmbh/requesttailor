// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ReviewApi` of the desktop app (wave 12, strand E; src-tauri/src/review.rs). Interface
// placeholder: the Tauri commands are registered and refuse; the strand calls them here.

import { stubReview, type ReviewApi } from "@requesttailor/ui";

import type { Call, Command } from "./backend";

export function tauriReview(call: Call): ReviewApi {
  void call;
  return stubReview();
}

/** Commands of this strand no method calls yet; backend.test.ts leaves them out of its
 *  "every command is mapped" check. The strand empties the list as it maps them. */
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
