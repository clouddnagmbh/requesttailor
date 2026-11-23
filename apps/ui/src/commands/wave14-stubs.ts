// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14: the method groups of `GitApi` that the commit graph (strand F) and the Git wizards
// (strand G) added. Every shell implements them in its own files; the placeholders that stood
// here until those strands merged are gone.

import type { GitApi } from "./backend";

/** Wave 14: the commit-graph methods of `GitApi` (strand F). */
export type GitGraphMethods = Pick<GitApi, "graph" | "refs" | "changes" | "fileDiff">;
/** Wave 14: the wizard methods of `GitApi` (strand G). */
export type GitActionMethods = Pick<GitApi, "remotes" | "stashes" | "plan" | "apply" | "lastAction" | "undo">;
