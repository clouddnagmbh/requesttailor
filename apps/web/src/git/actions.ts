// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the Git wizards' backend of the web app (`GitApi.plan`, `apply`, `undo`,
// remotes, stashes) with isomorphic-git; semantic merges through rt-wasm as in `repo.ts`, network
// through the companion. Placeholder until strand G.

import { stubGitActions, type GitActionMethods } from "@requesttailor/ui";

import type { WebGitContext } from "./graph";

export function webGitActions(ctx: WebGitContext): GitActionMethods {
  void ctx;
  return stubGitActions();
}
