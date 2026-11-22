// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `MergeApi` of the web app (wave 12, strand B), over rt-wasm's merge
// (crates/rt-wasm/src/merges_js.rs), so the browser merges exactly like the desktop app and
// `rt merge-driver`.
//
// - The pull (./git/repo.ts, `mergeUpstream`) asks `collectionMerger()` for the merge of the
//   collection's files and gives isomorphic-git a merge driver that answers with it. The
//   merger is registered here when the backend is built; the core loads on the first pull.
// - `conflicts` recomputes the field conflicts from the conflicted files (base, ours, theirs
//   of the merge in progress) and the collection at HEAD; nothing is stored, so a merge
//   started on the command line shows the same.
// - `resolve` refuses with unsaved changes, writes what the choices decide, marks those files
//   resolved (`git add`) and reopens the collection; the next commit concludes the merge.

import { BackendError, type ConflictChoice, type FieldConflict, type MergeApi, type MergeState } from "@requesttailor/ui";

import type { Wave12Host } from "./wave12";
import { toBackendError, type CoreModule } from "./wasm";
import { directoryFs } from "./git/fs";

/** Files as `[path, text]` pairs, the way the core takes them. */
export type TextFiles = [string, string][];

/** One conflicted file of the merge in progress (`rt_app::merges::ConflictStages`). */
export interface ConflictStages {
  path: string;
  base: string | null;
  ours: string | null;
  theirs: string | null;
}

/** The merge of a pull (`mergeCollections`). */
export interface CollectionMerge {
  files: TextFiles;
  conflicts: FieldConflict[];
  text_conflicts: string[];
  /** Working-tree text of conflicted files: comments at conflicted fields, or Git's markers. */
  marked: TextFiles;
}

/** The merge's part of the rt-wasm module. */
export interface CoreMergeModule {
  mergeCollections(base: TextFiles, ours: TextFiles, theirs: TextFiles): CollectionMerge;
  mergeState(context: TextFiles, stages: ConflictStages[], merging: boolean): MergeState;
  mergeResolution(context: TextFiles, stages: ConflictStages[], choices: ConflictChoice[]): [string, string | null][];
  mergeText(base: string | null, ours: string, theirs: string): { text: string; clean: boolean };
}

/** The semantic merge for ./git/repo.ts. */
export interface CollectionMerger {
  merge(base: TextFiles, ours: TextFiles, theirs: TextFiles): Promise<CollectionMerge>;
  /** The line merge of a file outside the collection. */
  text(base: string | null, ours: string, theirs: string): Promise<{ text: string; clean: boolean }>;
}

let merger: CollectionMerger | null = null;

/** The merger the pull uses; `null` until a web backend registered one (unit tests). */
export function collectionMerger(): CollectionMerger | null {
  return merger;
}

/** Registers the merger (the web backend does; tests may pass their own or `null`). */
export function setCollectionMerger(m: CollectionMerger | null): void {
  merger = m;
}

/** The merger on top of a loaded (or loading) core. */
export function coreMerger(load: () => Promise<CoreModule>): CollectionMerger {
  const call = async <T>(f: (core: CoreMergeModule) => T): Promise<T> => {
    const core = (await load()) as unknown as CoreMergeModule;
    try {
      return f(core);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  return {
    merge: (base, ours, theirs) => call((c) => c.mergeCollections(base, ours, theirs)),
    text: (base, ours, theirs) => call((c) => c.mergeText(base, ours, theirs)),
  };
}

export function webMerges(w: Wave12Host): MergeApi {
  const load = () => w.host.deps.loadCore();
  setCollectionMerger(coreMerger(load));
  const repo = () => ({ fs: directoryFs(w.host.session().handle), dir: "/", cache: {} });
  const ops = () => import("./git/repo");
  const core = async () => (await load()) as unknown as CoreMergeModule;
  const guard = async <T>(f: () => Promise<T>): Promise<T> => {
    try {
      return await f();
    } catch (e) {
      throw toBackendError(e);
    }
  };

  return {
    conflicts: () =>
      guard(async () => {
        const sides = await (await ops()).mergeSides(repo());
        if (sides.stages.length === 0) return { merging: sides.merging, conflicts: [], text_conflicts: [] };
        return (await core()).mergeState(sides.context, sides.stages, sides.merging);
      }),
    resolve: (choices) =>
      guard(async () => {
        const session = w.host.session();
        if (session.ws.dirty().length > 0) {
          throw new BackendError("git", "Save your changes before you resolve a conflict.");
        }
        const o = await ops();
        const r = repo();
        const sides = await o.mergeSides(r);
        if (!sides.merging) throw new BackendError("git", "No merge is in progress.");
        const files = (await core()).mergeResolution(sides.context, sides.stages, choices);
        await o.writeResolved(r, files);
        await w.host.openHandle(session.handle);
      }),
  };
}
