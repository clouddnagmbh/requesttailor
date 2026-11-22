// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `DiffApi` of the web app (wave 12, strand A): the semantic diff of two sets of collection
// files, computed by the core (`semanticDiff` of rt-wasm, i.e. `rt_app::diffs::diff`, the same
// code as the desktop app and `rt diff`). The files come from the web Git (`treeAt`) and, for
// the working state, from the collection's folder as it is on disk.

import { BackendError, type CollectionDiff, type DiffApi, type GitTree } from "@requesttailor/ui";

import { readDirectory } from "./fs/directory";
import { toBackendError } from "./wasm";
import type { Wave12Host } from "./wave12";

/** The diff's part of the rt-wasm module. */
export interface CoreDiffModule {
  semanticDiff(old: [string, string][], next: [string, string][]): CollectionDiff;
}

type Files = [string, string][];

const decoder = new TextDecoder("utf-8", { fatal: true, ignoreBOM: true });

function treeFiles(tree: GitTree | null): Files {
  return tree ? Object.entries(tree.files) : [];
}

/** The `.yml` files of the folder that are text, as `GitTree` holds them. */
export async function workingFiles(root: FileSystemDirectoryHandle): Promise<Files> {
  const out: Files = [];
  for (const [path, bytes] of await readDirectory(root)) {
    if (!path.endsWith(".yml")) continue;
    try {
      out.push([path, decoder.decode(bytes)]);
    } catch {
      // Not UTF-8: not a collection file.
    }
  }
  return out;
}

export function webDiffs(w: Wave12Host): DiffApi {
  const core = async (): Promise<CoreDiffModule> => (await w.host.deps.loadCore()) as unknown as CoreDiffModule;
  const compute = async (old: Files, next: Files): Promise<CollectionDiff> => {
    try {
      return (await core()).semanticDiff(old, next);
    } catch (e) {
      throw toBackendError(e);
    }
  };
  /** `rev`'s files; a missing revision (no commit yet) is an empty collection. */
  const at = async (rev: string): Promise<Files> => {
    try {
      return treeFiles(await w.git.treeAt(rev));
    } catch (e) {
      if (e instanceof BackendError && e.kind === "git") return [];
      throw e;
    }
  };
  return {
    working: async () => {
      const head = await at("HEAD");
      return compute(head, await workingFiles(w.host.session().handle));
    },
    commit: async (rev) => {
      const next = treeFiles(await w.git.treeAt(rev));
      return compute(await at(`${rev}^`), next);
    },
    between: async (from, to) => compute(treeFiles(await w.git.treeAt(from)), treeFiles(await w.git.treeAt(to))),
  };
}
