// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Repositories in the collection set of the web app (wave 15, strand A): `RepositoryApi` over
// `WebCollectionSet` (./collections.ts, which keeps the roots, the repositories and the chosen
// one) and the web Git (./git). The browser grants no access above the folder the user picks:
// a folder in Git without a collection is a repository root only if it holds `.git` itself.
//
// - `open` is the folder picker; the folder joins the set with its collections, or as a
//   repository root if it holds none.
// - `activate` chooses the repository: every `GitApi` command then acts on its whole work tree.
// - `summary` is read-only and does not wait for a running Git operation.
// - `clone` and `init` add the new folder as a root next to the others (the old `git.clone` of
//   the Git panel still opens it alone). Network goes through the companion, as for any clone.

import {
  BackendError,
  type CollectionSet,
  type GitCloneRequest,
  type GitOperation,
  type GitRepoSummary,
  type RepositoryApi,
  type RepositoryEntry,
} from "@requesttailor/ui";

import { same, type WebCollectionSet } from "./collections";
import type { RepositoryFolders } from "./git";

export interface WebRepositoriesDeps {
  collections: WebCollectionSet;
  /** The folder picker (`id` remembers where it started); `null` if cancelled. */
  pick(id: string): Promise<FileSystemDirectoryHandle | null>;
  folders: RepositoryFolders;
  /** Switches the repository in a folder to a branch. */
  checkout(folder: FileSystemDirectoryHandle, branch: string): Promise<void>;
}

async function exists(dir: FileSystemDirectoryHandle, path: string): Promise<boolean> {
  let at = dir;
  const parts = path.split("/");
  try {
    for (const part of parts.slice(0, -1)) at = await at.getDirectoryHandle(part);
    const last = parts[parts.length - 1]!;
    try {
      await at.getFileHandle(last);
      return true;
    } catch {
      await at.getDirectoryHandle(last);
      return true;
    }
  } catch {
    return false;
  }
}

/** What is half done in the repository in `folder` (the files git itself leaves). */
export async function operationOf(folder: FileSystemDirectoryHandle, merging: boolean): Promise<GitOperation | undefined> {
  for (const [file, op] of [
    [".git/MERGE_HEAD", "merge"],
    [".git/CHERRY_PICK_HEAD", "cherry-pick"],
    [".git/REVERT_HEAD", "revert"],
  ] as const) {
    if (await exists(folder, file)) return op;
  }
  if ((await exists(folder, ".git/rebase-merge")) || (await exists(folder, ".git/rebase-apply"))) return "rebase";
  return merging ? "merge" : undefined;
}

export function webRepositories(deps: WebRepositoriesDeps): RepositoryApi {
  const { collections } = deps;

  const handleOf = (id: string): FileSystemDirectoryHandle => {
    const handle = collections.repositoryHandle(id);
    if (!handle) throw new BackendError("shell", `No repository ${id} in the sidebar.`);
    return handle;
  };

  /** The repository a new folder ended up in, active. */
  async function added(handle: FileSystemDirectoryHandle): Promise<RepositoryEntry> {
    const set: CollectionSet = await collections.addFolder(handle);
    // The folder is the repository's work tree: the entry whose folder it is.
    for (const r of set.repositories) {
      const h = collections.repositoryHandle(r.id);
      if (h && (await same(h, handle))) {
        if (r.collections.length === 0) collections.chooseRepository(r.id);
        return r;
      }
    }
    const last = set.repositories.at(-1);
    if (!last) throw new BackendError("git", `"${handle.name}" is not a Git repository.`);
    return last;
  }

  return {
    async open() {
      return collections.openRepository();
    },

    async activate(id) {
      return collections.chooseRepository(id);
    },

    async summary(id): Promise<GitRepoSummary> {
      const entry = collections.repositoryEntry(id);
      if (!entry) throw new BackendError("shell", `No repository ${id} in the sidebar.`);
      const folder = handleOf(id);
      const status = await deps.folders.statusAt(folder);
      const operation = await operationOf(folder, status.merging);
      return {
        id,
        branch: status.branch,
        upstream: status.upstream,
        ahead: status.ahead,
        behind: status.behind,
        changes: status.files.length,
        ...(operation ? { operation } : {}),
      };
    },

    async clone(request: GitCloneRequest) {
      // The browser picks the folder itself: a path in `request.parent` cannot be read.
      const folder = await deps.folders.cloneFolder(request.url, request.name);
      if (!folder) return null;
      if (request.branch?.trim()) await deps.checkout(folder, request.branch.trim());
      return added(folder);
    },

    async init() {
      const folder = await deps.pick("requesttailor-init-repository");
      if (!folder) return null;
      await deps.folders.initFolder(folder);
      return added(folder);
    },
  };
}
