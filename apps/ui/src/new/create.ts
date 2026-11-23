// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What "Create" in the "New…" dialog does (wave 14, strand B), per kind. Everything is unsaved,
// like any edit: requests, folders, environments and MCP tests are staged in the target
// collection (made active first), a collection is created through `AppState.createCollection`.

import type { AppState } from "../state/app.svelte";
import {
  folderChildren,
  isRequestKind,
  mcpTest,
  newCollection,
  newEnvironment,
  requestSpec,
  validate,
  type NewForm,
} from "./forms";

export type CreateOutcome =
  /** Done; the dialog closes. */
  | { ok: true }
  /** The user cancelled a step on the way (the folder picker of "Elsewhere…"); stay open. */
  | { ok: false; cancelled: true }
  | { ok: false; error: string };

const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** Joins a folder path and a new segment. */
const join = (folder: string, name: string) => (folder === "" ? name : `${folder}/${name}`);

export async function createNew(app: AppState, form: NewForm): Promise<CreateOutcome> {
  if (form.kind === "collection") {
    const errors = validate(form, { collections: app.collections });
    const first = Object.values(errors)[0];
    if (first) return { ok: false, error: first };
    app.error = null;
    const info = await app.createCollection(newCollection(form));
    if (info) return { ok: true };
    return app.error ? { ok: false, error: app.error } : { ok: false, cancelled: true };
  }

  if (!form.collection) return { ok: false, error: "Open or create a collection first." };
  if (form.collection !== app.activeCollection) {
    await app.activate(form.collection);
    if (form.collection !== app.activeCollection) {
      return { ok: false, error: app.error ?? "That collection cannot be opened right now." };
    }
  }

  // Checked again against the collection as it is now (it may have changed since the form opened).
  const errors = validate(form, {
    environments: app.environments.map((e) => e.name),
    siblings: form.kind === "folder" ? folderChildren(app.tree, form.folder) : undefined,
  });
  const first = Object.values(errors)[0];
  if (first) return { ok: false, error: first };

  try {
    switch (form.kind) {
      case "folder":
        await app.backend.createFolder(join(form.folder, form.name.trim()));
        await app.reload();
        break;
      case "environment": {
        const from = form.copyFrom ? (app.environments.find((e) => e.name === form.copyFrom) ?? null) : null;
        const env = newEnvironment(form.name, from);
        await app.backend.updateEnvironment(env);
        await app.reloadEnvironments();
        app.setEnvironment(env.name);
        break;
      }
      case "mcp-test":
        await app.backend.mcp.saveTest(null, mcpTest(form));
        app.dirty = await app.backend.dirty();
        app.show({ kind: "mcp" });
        break;
      default: {
        if (!isRequestKind(form.kind)) return { ok: false, error: "Unknown kind." };
        app.error = null;
        const before = app.active;
        await app.create(form.folder, form.name.trim());
        const path = app.active;
        if (app.error || !path || path === before) return { ok: false, error: app.error ?? "The request was not created." };
        const draft = app.drafts[path];
        if (draft) await app.edit(path, requestSpec(draft, form));
      }
    }
  } catch (e) {
    return { ok: false, error: message(e) };
  }
  return app.error ? { ok: false, error: app.error } : { ok: true };
}
