// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the web app (wave 12, strand C), over rt-wasm's compare functions
// (crates/rt-wasm/src/compare_js.rs): `Workspace.runAtRevision`, `compareRevisions` and
// `bisect`. The trees and commits come from the web Git (isomorphic-git, no checkout); every
// request goes out like a single send: with today's secret values, through the companion when
// it is on, with the same services. Each run's results go to the time travel under the commit
// it ran at (./timeline.ts, `recordAtRevision`).

import {
  BackendError,
  type BisectOutcome,
  type BisectRequest,
  type BisectStep,
  type CompareApi,
  type CompareRequest,
  type GitCommit,
  type GitTree,
  type RevisionRun,
  type RevisionRunRequest,
  type RunComparison,
  type TestRecord,
} from "@requesttailor/ui";

import type { CoreSendServices } from "./send-services";
import { toBackendError, type CompanionCredentials, type CoreSendInput } from "./wasm";
import { recordAtRevision } from "./timeline";
import type { Wave12Host } from "./wave12";

/** Gets the time travel records of a finished run at a revision. */
export type OnRecords = (oid: string, records: TestRecord[]) => void;

/** The compare part of rt-wasm's `Workspace`. */
export interface CoreCompareWorkspace {
  runAtRevision(
    request: RevisionRunRequest,
    commit: GitCommit,
    tree: GitTree,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    cancelled: () => boolean,
    onRecords: OnRecords,
  ): Promise<RevisionRun>;
  compareRevisions(
    request: CompareRequest,
    fromCommit: GitCommit,
    fromTree: GitTree,
    toCommit: GitCommit,
    toTree: GitTree,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    cancelled: () => boolean,
    onRecords: OnRecords,
  ): Promise<RunComparison>;
  bisect(
    request: BisectRequest,
    candidates: GitCommit[],
    treeAt: (oid: string) => Promise<GitTree>,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    onStep: (step: BisectStep) => void,
    cancelled: () => boolean,
    onRecords: OnRecords,
  ): Promise<BisectOutcome>;
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

/** Most commits a bisect looks at between good and bad. */
const CANDIDATE_LIMIT = 1_000_000;

export function webCompare(w: Wave12Host, onRecords: OnRecords = recordAtRevision): CompareApi {
  // One compare or bisect at a time; `cancel` flips the flag the core polls.
  let active: { cancelled: boolean } | null = null;

  const workspace = () => w.host.session().ws as unknown as CoreCompareWorkspace;

  /** The commit `rev` names with its files. */
  const at = async (rev: string): Promise<[GitCommit, GitTree]> => {
    const tree = await w.git.treeAt(rev);
    const [commit] = await w.git.commits(null, tree.oid, 1);
    if (!commit) throw new BackendError("git", `${rev} does not name a commit.`);
    return [commit, tree];
  };

  const exclusive = async <T>(job: (token: { cancelled: boolean }) => Promise<T>): Promise<T> => {
    if (active) throw new BackendError("shell", "A compare or bisect is already in progress.");
    const token = { cancelled: false };
    active = token;
    try {
      return await guard(() => job(token));
    } finally {
      active = null;
    }
  };

  return {
    run: (request, input) =>
      exclusive(async (token) => {
        const ws = workspace();
        const [commit, tree] = await at(request.rev);
        const context = await w.sendContext(input);
        return ws.runAtRevision(request, commit, tree, context.input, context.companion, context.services, () => token.cancelled, onRecords);
      }),
    compare: (request, input) =>
      exclusive(async (token) => {
        const ws = workspace();
        const [from, to] = [await at(request.from), await at(request.to)];
        const context = await w.sendContext(input);
        return ws.compareRevisions(
          request,
          from[0],
          from[1],
          to[0],
          to[1],
          context.input,
          context.companion,
          context.services,
          () => token.cancelled,
          onRecords,
        );
      }),
    bisect: (request, input, onStep) =>
      exclusive(async (token) => {
        const ws = workspace();
        const candidates = await w.git.commits(request.good, request.bad, CANDIDATE_LIMIT);
        const context = await w.sendContext(input);
        return ws.bisect(
          request,
          candidates,
          (oid) => w.git.treeAt(oid),
          context.input,
          context.companion,
          context.services,
          onStep,
          () => token.cancelled,
          onRecords,
        );
      }),
    cancel: async () => {
      if (active) active.cancelled = true;
    },
  };
}
