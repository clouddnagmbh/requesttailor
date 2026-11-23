// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16 interface: `ExtensionContext.suggest`, a suggested change shown as a diff before it is
// applied. Strand B builds the inline diff and apply component (`SuggestedEditView.svelte`, also
// used by the app's own assistant features and by `EditorHandle.propose`), applying through the
// existing paths (`AppState.applyExternal`, `Backend.createRequest`, folder scripts,
// `Backend.companions.stage`) and the undo of an applied suggestion. Until then: `unsupported`.

import type { Backend } from "../commands/backend";
import { BackendError } from "../commands/backend";
import type { SuggestedEdit } from "../extensions";
import type { AppState } from "../state/app.svelte";

export function createSuggest(_backend: Backend, _app: AppState): (edit: SuggestedEdit) => Promise<boolean> {
  return () => Promise.reject(new BackendError("unsupported", "Suggested edits arrive in wave 16 (strand C)."));
}
