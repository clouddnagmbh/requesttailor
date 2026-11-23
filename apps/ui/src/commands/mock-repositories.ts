// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Repositories in the mock set (wave 15, strand A). The demo folder `mock://demo` is one
// repository that holds both collections. "Open repository" adds a folder in Git without a
// collection as a repository root (the mock's folder picker always picks `nextFolder`, the way
// `MockGit.picked` stands for the desktop's folder picker); `init` adds a new one. Clone
// validates the address like `MockGit.clone` (its call log and credential rules stay the one
// the clone tests know) and shows the cloned collections: the demo folder.

import type { GitApi, RepositoryApi } from "./backend";
import { BackendError } from "./backend";
import type { MockCollections } from "./mock-collections";
import type { GitRepoSummary, RepositoryEntry } from "./types";

export interface MockRepositoriesHost {
  collections: MockCollections;
  /** The mock Git (created after this object, so asked for lazily). */
  git: () => GitApi;
}

export interface MockRepositories extends RepositoryApi {
  /** The folder "Open repository" picks next (a folder name; `null` cancels). */
  nextFolder: string | null;
  /** The folder `init` picks next (`null` cancels). */
  nextInit: string | null;
}

export function mockRepositories(host: MockRepositoriesHost): MockRepositories {
  const { collections } = host;
  const find = async (id: string): Promise<RepositoryEntry> => {
    const entry = (await collections.current()).repositories.find((r) => r.id === id);
    if (!entry) throw new BackendError("shell", `no repository ${id} in the sidebar`);
    return entry;
  };
  const addRoot = async (name: string, summary: Partial<MockRepoSummary> = {}): Promise<RepositoryEntry> => {
    const id = `mock://${name}`;
    if (!collections.repoRoots.some((r) => r.id === id) && !(await collections.current()).repositories.some((r) => r.id === id)) {
      collections.repoRoots.push({ id, name, summary: { branch: "main", upstream: null, ahead: 0, behind: 0, changes: 2, ...summary } });
    }
    collections.chosenRepo = id;
    return find(id);
  };
  const api: MockRepositories = {
    nextFolder: "notes",
    nextInit: "new-repo",

    async open() {
      const name = api.nextFolder;
      if (!name) return null;
      await addRoot(name);
      return collections.current();
    },

    async activate(id) {
      const entry = await find(id);
      collections.chosenRepo = entry.id;
      return entry;
    },

    async summary(id): Promise<GitRepoSummary> {
      const entry = await find(id);
      const root = collections.repoRoots.find((r) => r.id === entry.id);
      if (root) return { id: entry.id, ...root.summary, operation: undefined };
      // A repository with collections: what the mock Git says about the demo working tree.
      const s = await host.git().status();
      return {
        id: entry.id,
        branch: s.branch,
        upstream: s.upstream,
        ahead: s.ahead,
        behind: s.behind,
        changes: s.files.length,
        ...(s.merging ? { operation: "merge" as const } : {}),
      };
    },

    async clone(request) {
      await host.git().clone(request.url);
      // The cloned collections are shown (the mock clone opens the demo folder).
      return find("mock://demo");
    },

    async init() {
      const name = api.nextInit;
      if (!name) return null;
      return addRoot(name, { branch: null, changes: 0 });
    },
  };
  return api;
}

type MockRepoSummary = { branch: string | null; upstream: string | null; ahead: number; behind: number; changes: number };
