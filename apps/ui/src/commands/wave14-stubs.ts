// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: a placeholder `CollectionSetApi` over the single open collection, until
// strand A replaces it in each shell's own file (desktop `src/collections.ts`, web
// `src/collections.ts`, mock `mock-collections.ts`). It knows one collection: reads go to the
// existing single-collection commands, set changes reject with `unsupported`.

import type { Backend, CollectionSetApi, GitApi } from "./backend";
import { BackendError } from "./backend";

const notYet = () =>
  Promise.reject(new BackendError("unsupported", "Several collections in the sidebar arrive in wave 14 (strand A)."));

export function stubCollections(single: Pick<Backend, "tree" | "dirty" | "save" | "search">): CollectionSetApi {
  return {
    current: async () => ({ roots: [], collections: [], active: null }),
    add: notYet,
    removeRoot: notYet,
    activate: notYet,
    tree: () => single.tree(),
    dirty: () => single.dirty(),
    saveAll: async () => {
      const changes = await single.save();
      return changes.length > 0 ? [{ collection: "", changes }] : [];
    },
    search: async (query, limit) => (await single.search(query, limit)).map((hit) => ({ ...hit, collection: "" })),
    create: notYet,
  };
}

/** Wave 14: the commit-graph methods of `GitApi` (strand F). */
export type GitGraphMethods = Pick<GitApi, "graph" | "refs" | "changes" | "fileDiff">;
/** Wave 14: the wizard methods of `GitApi` (strand G). */
export type GitActionMethods = Pick<GitApi, "remotes" | "stashes" | "plan" | "apply" | "lastAction" | "undo">;

const later = (strand: string) => () =>
  Promise.reject(new BackendError("unsupported", `This Git view arrives in wave 14 (strand ${strand}).`));

/** Placeholder until strand F implements the graph in each shell's own file (desktop
 *  `src/git-graph.ts` over the Tauri commands, web `src/git/graph.ts`, mock `mock-git-graph.ts`). */
export function stubGitGraph(): GitGraphMethods {
  const no = later("F");
  return { graph: no, refs: no, changes: no, fileDiff: no };
}

/** Placeholder until strand G implements the wizards' backend (desktop `src/git-actions.ts`, web
 *  `src/git/actions.ts`, mock `mock-git-actions.ts`). Nothing to undo yet. */
export function stubGitActions(): GitActionMethods {
  const no = later("G");
  return { remotes: no, stashes: no, plan: no, apply: no, lastAction: async () => null, undo: no };
}
