// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14 interface: placeholders for the Git graph (strand F) and the Git wizards (strand G),
// until each shell implements them in its own file. The collection set (strand A) has its own
// implementation in every shell since its merge.

import type { GitApi } from "./backend";
import { BackendError } from "./backend";

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
