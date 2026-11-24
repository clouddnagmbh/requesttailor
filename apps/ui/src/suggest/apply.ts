// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Prepares and applies a `SuggestedEdit` (wave 16, strand C). `prepare` reads the current state
// and builds what the user reviews; `apply` writes the change through the paths the app
// already has (`AppState.applyExternal`, `AppState.create`, `Backend.companions.stage`, folder
// scripts) and hands back the undo. Nothing is saved: an applied suggestion is an unsaved edit
// like any other. A suggestion made against a state that has changed since is refused, never
// merged.

import { BackendError, type Backend } from "../commands/backend";
import type { Environment, RequestSpec } from "../commands/types";
import type { SuggestedEdit, SuggestionNote } from "../extensions";
import type { AppState } from "../state/app.svelte";
import { requestBlocks, scriptBlocks, textBlock, type PreviewBlock } from "./model";

export interface Prepared {
  /** What the dialog says it is: "Change request", "New request", … */
  title: string;
  /** The path or file it concerns. */
  subtitle: string;
  note?: SuggestionNote;
  blocks: PreviewBlock[];
  /** The change can be undone after applying. */
  undoable: boolean;
  /** `SuggestedEdit.dismissKey`: the dialog offers "Don't suggest this again". */
  dismissKey?: string;
  /** Rejects with kind `stale` when the target changed after `prepare`, without changing
   *  anything. `apply` checks the same; a batch checks all its parts before it applies one. */
  verify(): Promise<void>;
  /** Writes the change. Rejects with kind `stale` when the target changed after `prepare`. */
  apply(): Promise<Applied>;
}

export interface Applied {
  /** Reverts the change if the target still is as `apply` left it; `false` if it is not (or
   *  nothing changed it). `null` for changes that cannot be reverted. */
  undo: (() => Promise<boolean>) | null;
  /** Whether `undo` would put the change back now (the target still is as `apply` left it);
   *  a batch undoes only if every part can. Present whenever `undo` is. */
  canUndo?: () => Promise<boolean>;
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
  return { ...(await prepareKind(backend, app, edit, note)), dismissKey: edit.dismissKey };
}

async function prepareKind(backend: Backend, app: AppState, edit: SuggestedEdit, note: SuggestionNote | undefined): Promise<Prepared> {
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
        async verify() {
          const now = await currentRequest(backend, app, edit.path);
          if (!same(now, base)) throw stale(`The request ${edit.path}`);
        },
        async apply() {
          await (this as Prepared).verify();
          await app.applyExternal(edit.path, next);
          const left = async () => {
            const current = await currentRequest(backend, app, edit.path).catch(() => null);
            return current !== null && same(current, next);
          };
          return {
            canUndo: left,
            async undo() {
              if (!(await left())) return false;
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
        async verify() {},
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
      const access = backend.folderScripts;
      const base = await access.get(edit.path);
      const next = edit.scripts.map((s) => ({ ...s }));
      return {
        title: edit.path ? "Change folder scripts" : "Change collection scripts",
        subtitle: edit.path || "Collection",
        note,
        blocks: scriptBlocks(base, next),
        undoable: true,
        async verify() {
          if (!same(await access.get(edit.path), base)) throw stale(edit.path ? `The scripts of ${edit.path}` : "The collection scripts");
        },
        async apply() {
          await (this as Prepared).verify();
          await access.set(edit.path, next);
          await app.refreshDirty();
          return {
            canUndo: async () => same(await access.get(edit.path), next),
            async undo() {
              if (!same(await access.get(edit.path), next)) return false;
              await access.set(edit.path, base);
              await app.refreshDirty();
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
        async verify() {
          if ((await backend.companions.read(edit.path)) !== base) throw stale(edit.path);
        },
        async apply() {
          await (this as Prepared).verify();
          await backend.companions.stage(edit.path, edit.text);
          await app.refreshDirty();
          return {
            canUndo: base === null ? undefined : async () => (await backend.companions.read(edit.path)) === edit.text,
            // A file that did not exist stays (an empty file); only changes to an existing one
            // can be put back.
            undo:
              base === null
                ? null
                : async () => {
                    if ((await backend.companions.read(edit.path)) !== edit.text) return false;
                    await backend.companions.stage(edit.path, base);
                    await app.refreshDirty();
                    return true;
                  },
          };
        },
      };
    }
    case "environment": {
      await app.settled();
      const base = (await backend.environments()).find((e) => e.name === edit.environment);
      if (!base) throw new BackendError("not-found", `The environment ${edit.environment} does not exist (any more).`);
      const have = new Set(base.variables.map((v) => v.name));
      const added: Environment["variables"] = [];
      for (const v of edit.variables) {
        if (!v.name.trim() || have.has(v.name)) continue;
        have.add(v.name);
        const secret = v.secret === true;
        added.push({ name: v.name, value: secret ? null : (v.value ?? ""), enabled: true, secret });
      }
      const next: Environment = { ...clone(base), variables: [...clone(base.variables), ...added] };
      const current = async () => (await backend.environments()).find((e) => e.name === edit.environment) ?? null;
      const reload = async () => {
        await app.reloadEnvironments();
        await app.refreshDirty();
      };
      return {
        title: "Change environment",
        subtitle: edit.environment,
        note,
        blocks: added.map((v) => ({
          label: `Variable › ${v.name}`,
          old: null,
          next: v.secret ? "(secret, no value yet)" : (v.value ?? ""),
          code: false,
        })),
        undoable: true,
        async verify() {
          if (!same(await current(), base)) throw stale(`The environment ${edit.environment}`);
        },
        async apply() {
          await (this as Prepared).verify();
          await backend.updateEnvironment(next);
          await reload();
          const left = async () => same(await current(), next);
          return {
            canUndo: left,
            async undo() {
              if (!(await left())) return false;
              await backend.updateEnvironment(base);
              await reload();
              return true;
            },
          };
        },
      };
    }
    case "batch":
      return prepareBatch(backend, app, edit, note);
  }
}

/** One dialog for several edits (wave 17). Every part is prepared against the current state;
 *  nothing is written before every part still matches (`verify`), the parts that change
 *  existing things go first and new requests last, and a failure puts back what was already
 *  applied, so the batch is applied entirely or not at all. One undo step puts all parts
 *  back, or none if any part was changed since. A batch with a new request cannot be undone
 *  (a new request cannot be removed). */
async function prepareBatch(
  backend: Backend,
  app: AppState,
  edit: Extract<SuggestedEdit, { kind: "batch" }>,
  note: SuggestionNote | undefined,
): Promise<Prepared> {
  if (edit.edits.length === 0) throw new BackendError("invalid", "The suggestion has no changes.");
  const seen = new Set<string>();
  for (const part of edit.edits) {
    if ((part as { kind: string }).kind === "batch") throw new BackendError("invalid", "A batch cannot contain a batch.");
    const key = part.kind === "new-request" ? null : `${part.kind}:${part.kind === "environment" ? part.environment : "path" in part ? part.path : ""}`;
    if (key !== null) {
      if (seen.has(key)) throw new BackendError("invalid", `The suggestion changes ${key.slice(key.indexOf(":") + 1) || "the collection"} twice.`);
      seen.add(key);
    }
  }
  await activate(app, edit.collection);
  // The parts belong to the batch's collection; their own `collection` is ignored.
  const parts: Prepared[] = [];
  for (const part of edit.edits) parts.push(await prepare(backend, app, { ...part, collection: undefined }));
  const blocks = parts.flatMap((p) => p.blocks.map((b) => ({ ...b, label: `${p.subtitle} › ${b.label}` })));
  const undoable = parts.every((p) => p.undoable);
  // Existing things first, creations last.
  const order = parts.map((p, i) => i).sort((x, y) => Number(edit.edits[x].kind === "new-request") - Number(edit.edits[y].kind === "new-request"));
  return {
    title: parts.length === 1 ? parts[0].title : `Apply ${parts.length} changes`,
    subtitle: parts.map((p) => p.subtitle).join(", "),
    note,
    blocks,
    undoable,
    async verify() {
      for (const p of parts) await p.verify();
    },
    async apply() {
      await (this as Prepared).verify();
      const applied: Applied[] = [];
      try {
        for (const i of order) applied.push(await parts[i].apply());
      } catch (e) {
        for (const done of applied.reverse()) await done.undo?.().catch(() => false);
        throw e;
      }
      if (!undoable) return { undo: null };
      const canUndo = async () => {
        for (const a of applied) if (!a.canUndo || !(await a.canUndo())) return false;
        return true;
      };
      return {
        canUndo,
        async undo() {
          if (!(await canUndo())) return false;
          for (const a of [...applied].reverse()) if (!(await a.undo!())) return false;
          return true;
        },
      };
    },
  };
}
