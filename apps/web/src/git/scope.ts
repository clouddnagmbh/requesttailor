// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14: a collection may sit in a subfolder of its repository. The web Git then runs at
// the repository's folder (`Repo.scope` is the collection's folder in it, like `Scope` in
// `crates/rt-git/src/util.rs`); these map between collection and repository paths.

import type { Repo } from "./repo";

/** Repository path of the collection path `rel` (a leading `/`: already relative to the
 *  repository). */
export function repoPath(repo: Repo, rel: string): string {
  if (rel.startsWith("/")) return rel.replace(/^\/+/, "");
  return repo.scope ? `${repo.scope}/${rel}` : rel;
}

/** Collection path of the repository path `p`, `null` outside the collection. */
export function scopedPath(repo: Repo, p: string): string | null {
  if (!repo.scope) return p;
  return p.startsWith(`${repo.scope}/`) ? p.slice(repo.scope.length + 1) : null;
}

/** Collection path of `p`, or `/p` for a file outside the collection. */
export function collectionPath(repo: Repo, p: string): string {
  return scopedPath(repo, p) ?? `/${p}`;
}
