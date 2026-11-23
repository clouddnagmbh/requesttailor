// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Repositories of a collection set (wave 15, strand A) for the shells that derive the set in
// TypeScript (web, mock): the same rules as `rt_app::git::repos::{repositories,
// active_repository}`. A repository that holds collections is listed in the order of the
// collections, then the repository roots (folders in Git without a collection); a repository
// reached through several roots is listed once, under the first. The active repository is the
// one chosen in the sidebar if it is still in the set, else the active collection's.

import type { CollectionEntry, CollectionSet, RepositoryEntry } from "./types";

/** A root that is a repository root: its id and the repository it is in. */
export interface RepoRoot {
  root: string;
  repo: string;
}

/** Name, display location and "linked worktree" of a repository id. */
export interface RepoDescription {
  name: string;
  location: string;
  linked: boolean;
}

/** The last path segment of a repository id (`/work/api` → `api`). */
export function repositoryName(id: string): string {
  return id.split(/[\\/]/).filter(Boolean).pop() ?? id;
}

const plain = (id: string): RepoDescription => ({ name: repositoryName(id), location: id, linked: false });

export function repositoriesOf(
  collections: readonly CollectionEntry[],
  repoRoots: readonly RepoRoot[] = [],
  describe: (id: string) => RepoDescription = plain,
): RepositoryEntry[] {
  const out: RepositoryEntry[] = [];
  const add = (id: string, root: string, collection?: string) => {
    const known = out.find((r) => r.id === id);
    if (known) {
      if (collection) known.collections.push(collection);
      return;
    }
    out.push({ id, ...describe(id), root, collections: collection ? [collection] : [] });
  };
  for (const c of collections) if (c.repo) add(c.repo, c.root, c.id);
  for (const r of repoRoots) add(r.repo, r.root);
  return out;
}

export function activeRepositoryOf(
  repositories: readonly RepositoryEntry[],
  collections: readonly CollectionEntry[],
  chosen: string | null | undefined,
  active: string | null | undefined,
): string | undefined {
  if (chosen && repositories.some((r) => r.id === chosen)) return chosen;
  return active ? (collections.find((c) => c.id === active)?.repo ?? undefined) : undefined;
}

export interface RepositoryOptions {
  repoRoots?: readonly RepoRoot[];
  /** The repository chosen in the sidebar. */
  chosen?: string | null;
  describe?: (id: string) => RepoDescription;
}

/** `repositories` and `active_repository` of a set from its collections and repository roots. */
export function withRepositories(
  set: Omit<CollectionSet, "repositories" | "active_repository">,
  options: RepositoryOptions = {},
): CollectionSet {
  const repositories = repositoriesOf(set.collections, options.repoRoots, options.describe);
  const active = activeRepositoryOf(repositories, set.collections, options.chosen, set.active);
  return active ? { ...set, repositories, active_repository: active } : { ...set, repositories };
}
