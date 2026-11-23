// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Prepares and applies a `SuggestedEdit` (wave 16, strand C). `prepare` reads the current state
// and builds what the user reviews; `apply` writes the change through the paths the app
// already has (`AppState.applyExternal`, `AppState.create`, `Backend.companions.stage`, folder
// scripts) and hands back the undo. Nothing is saved: an applied suggestion is an unsaved edit
// like any other. A suggestion made against a state that has changed since is refused, never
// merged.

import { BackendError, type Backend } from "../commands/backend";
import type { RequestSpec, Script } from "../commands/types";
import type { SuggestedEdit, SuggestionNote } from "../extensions";
import type { AppState } from "../state/app.svelte";
import { requestBlocks, scriptBlocks, textBlock, type PreviewBlock } from "./model";

/** Reading and replacing the scripts of a folder or of the collection (`path` `""`). The core
 *  backend has no such calls yet: a shell that has them provides them as `Backend.folderScripts`
 *  and folder-script suggestions work; without them they are refused with kind `unsupported`. */
export interface FolderScriptsAccess {
  get(path: string): Promise<Script[]>;
  set(path: string, scripts: Script[]): Promise<void>;
}

export const folderScriptsOf = (backend: Backend): FolderScriptsAccess | null =>
  (backend as Backend & { folderScripts?: FolderScriptsAccess }).folderScripts ?? null;

export interface Prepared {
  /** What the dialog says it is: "Change request", "New request", … */
  title: string;
  /** The path or file it concerns. */
  subtitle: string;
  note?: SuggestionNote;
  blocks: PreviewBlock[];
  /** The change can be undone after applying. */
  undoable: boolean;
  /** Writes the change. Rejects with kind `stale` when the target changed after `prepare`. */
  apply(): Promise<Applied>;
}

export interface Applied {
  /** Reverts the change if the target still is as `apply` left it; `false` if it is not (or
   *  nothing changed it). `null` for changes that cannot be reverted. */
  undo: (() => Promise<boolean>) | null;
}

const stale = (what: string) => new BackendError("stale", `${what} changed since the suggestion was made. Ask for a new one.`);
const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v)) as T;
const same = (a: unknown, b: unknown) => JSON.stringify(a) === JSON.stringify(b);

async function activate(app: AppState, collection: string | undefined): Promise<void> {
  if (collection && collection !== app.activeCollection) await app.activate(collection);
}

async function currentRequest(backend: Backend, app: AppState, path: string): Promise<RequestSpec> {
  await app.settled();
  const spec = await backend.getRequest(path).catch(() => null);
  if (!spec) throw new BackendError("not-found", `The request ${path} does not exist (any more).`);
  return spec;
}

export async function prepare(backend: Backend, app: AppState, edit: SuggestedEdit): Promise<Prepared> {
  await activate(app, edit.collection);
  const note = edit.note;
  switch (edit.kind) {
    case "request": {
      const base = await currentRequest(backend, app, edit.path);
      const next = clone(edit.spec);
      return {
        title: "Change request",
        subtitle: edit.path,
        note,
        blocks: requestBlocks(base, next),
        undoable: true,
        async apply() {
          const now = await currentRequest(backend, app, edit.path);
          if (!same(now, base)) throw stale(`The request ${edit.path}`);
          await app.applyExternal(edit.path, next);
          return {
            async undo() {
              const current = await currentRequest(backend, app, edit.path).catch(() => null);
              if (!current || !same(current, next)) return false;
              await app.applyExternal(edit.path, base);
              return true;
            },
          };
        },
      };
    }
    case "new-request": {
      const next = clone(edit.spec);
      return {
        title: "New request",
        subtitle: edit.folder ? `in ${edit.folder}` : "in the collection root",
        note,
        blocks: requestBlocks(null, next),
        undoable: false,
        async apply() {
          await app.create(edit.folder, next.name || "New request");
          const path = app.active;
          if (!path) throw new BackendError("shell", "The new request could not be opened.");
          await app.applyExternal(path, next);
          // There is no way to remove a request from memory yet: discard it with the other
          // unsaved changes. The view says so.
          return { undo: null };
        },
      };
    }
    case "folder-scripts": {
      const access = folderScriptsOf(backend);
      if (!access) {
        throw new BackendError("unsupported", "This build cannot change folder or collection scripts yet.");
      }
      const base = await access.get(edit.path);
      const next = edit.scripts.map((s) => ({ ...s }));
      return {
        title: edit.path ? "Change folder scripts" : "Change collection scripts",
        subtitle: edit.path || "Collection",
        note,
        blocks: scriptBlocks(base, next),
        undoable: true,
        async apply() {
          if (!same(await access.get(edit.path), base)) throw stale(edit.path ? `The scripts of ${edit.path}` : "The collection scripts");
          await access.set(edit.path, next);
          return {
            async undo() {
              if (!same(await access.get(edit.path), next)) return false;
              await access.set(edit.path, base);
              return true;
            },
          };
        },
      };
    }
    case "file": {
      if (edit.path.endsWith(".yml")) {
        throw new BackendError("invalid", "Collection files (*.yml) are changed through requests, not as text files.");
      }
      const base = await backend.companions.read(edit.path);
      return {
        title: base === null ? "New file" : "Change file",
        subtitle: edit.path,
        note,
        blocks: [textBlock(edit.path, base, edit.text)],
        undoable: base !== null,
        async apply() {
          if ((await backend.companions.read(edit.path)) !== base) throw stale(edit.path);
          await backend.companions.stage(edit.path, edit.text);
          return {
            // A file that did not exist stays (an empty file); only changes to an existing one
            // can be put back.
            undo:
              base === null
                ? null
                : async () => {
                    if ((await backend.companions.read(edit.path)) !== edit.text) return false;
                    await backend.companions.stage(edit.path, base);
                    return true;
                  },
          };
        },
      };
    }
  }
}
