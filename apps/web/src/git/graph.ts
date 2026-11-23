// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: the commit graph of the web app (`GitApi.graph`, `refs`, `changes`,
// `fileDiff`) with isomorphic-git over the repository of the active collection. Placeholder
// until strand F.

import { stubGitGraph, type GitGraphMethods } from "@requesttailor/ui";

import type { WebHost } from "../web-backend";
import type { Repo } from "./repo";

/** What `index.ts` hands the wave-14 Git parts: the host, the repository of the active
 *  collection and the queue that runs one Git operation at a time (errors mapped). */
export interface WebGitContext {
  host: WebHost;
  repo(): Repo;
  run<T>(op: () => Promise<T>): Promise<T>;
}

export function webGitGraph(ctx: WebGitContext): GitGraphMethods {
  void ctx;
  return stubGitGraph();
}
