// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ExtensionContext.suggest` (wave 16, strand C): shows a suggested change as a diff in a
// dialog; the user applies or discards it. Applying writes an unsaved edit and offers an undo
// (`undo.svelte.ts`). Also used by the app's own assistant features. No model calls here.

import type { Backend } from "../commands/backend";
import type { SuggestedEdit } from "../extensions";
import type { AppState } from "../state/app.svelte";
import { prepare } from "./apply";
import SuggestDialog from "./SuggestDialog.svelte";

export function createSuggest(backend: Backend, app: AppState): (edit: SuggestedEdit) => Promise<boolean> {
  return async (edit) => {
    const prepared = await prepare(backend, app, edit);
    const result = await app.showDialog({
      title: "Review suggestion",
      component: SuggestDialog,
      props: { prepared },
      width: 760,
    });
    return result === true;
  };
}
