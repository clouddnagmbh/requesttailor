// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The `ExtensionContext` the shell hands to every extension (since wave 9; `ai` and `suggest`
// since wave 16). One place, so tests build the same context as the shell.

import { createExtensionAi } from "../ai/extension-ai";
import type { Backend } from "../commands/backend";
import type { ExtensionContext } from "../extensions";
import type { AppState } from "../state/app.svelte";
import { createSuggest, createSuggestOutcome } from "../suggest/suggest";

export function createExtensionContext(backend: Backend, app: AppState): ExtensionContext {
  return { backend, app, ai: createExtensionAi(backend, app), suggest: createSuggest(backend, app), suggestOutcome: createSuggestOutcome(backend, app) };
}
