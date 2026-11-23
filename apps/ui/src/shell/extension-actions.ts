// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Actions of a distribution's extensions in code editors, on requests, responses and runs, and
// the assistant panel (since wave 16): collected here for the places that show them. Strand B
// mounts them (CodeField and the script editors, request editor, response panel, runner and
// bisect results, the commit graph's selection of two revisions, the comparison and the diff of
// two revisions, a side panel of the main area).

import type {
  AppExtension,
  EditorTarget,
  ExtensionAction,
  ExtensionAssistant,
  ExtensionContext,
  ExtensionEditorAction,
  RequestActionTarget,
  ResponseActionTarget,
  RevisionRangeTarget,
  RunActionTarget,
} from "../extensions";
import type { ProvidedExtensions } from "./extension-tabs";

export interface ExtensionActionRef<A> {
  /** `<extension id>/<action id>`. */
  key: string;
  extension: string;
  action: A;
}

function collect<A extends { id: string }>(
  provided: ProvidedExtensions | null,
  place: string,
  get: (x: AppExtension, ctx: ExtensionContext) => readonly A[] | undefined,
): ExtensionActionRef<A>[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => {
    try {
      return (get(x, provided.ctx) ?? []).map((action) => ({ key: `${x.id}/${action.id}`, extension: x.id, action }));
    } catch (e) {
      console.warn(`RequestTailor: ${place} of extension ${x.id} failed:`, e);
      return [];
    }
  });
}

export function extensionEditorActions(provided: ProvidedExtensions | null, target: EditorTarget): ExtensionActionRef<ExtensionEditorAction>[] {
  return collect(provided, "editor actions", (x, ctx) => x.editorActions?.(ctx, target));
}

export function extensionRequestActions(provided: ProvidedExtensions | null, target: RequestActionTarget): ExtensionActionRef<ExtensionAction>[] {
  return collect(provided, "request actions", (x, ctx) => x.requestActions?.(ctx, target));
}

export function extensionResponseActions(provided: ProvidedExtensions | null, target: ResponseActionTarget): ExtensionActionRef<ExtensionAction>[] {
  return collect(provided, "response actions", (x, ctx) => x.responseActions?.(ctx, target));
}

export function extensionRunActions(provided: ProvidedExtensions | null, target: RunActionTarget): ExtensionActionRef<ExtensionAction>[] {
  return collect(provided, "run actions", (x, ctx) => x.runActions?.(ctx, target));
}

export function extensionRangeActions(provided: ProvidedExtensions | null, target: RevisionRangeTarget): ExtensionActionRef<ExtensionAction>[] {
  return collect(provided, "range actions", (x, ctx) => x.rangeActions?.(ctx, target));
}

/** The assistant panel the shell shows: the first extension's. */
export function extensionAssistant(provided: ProvidedExtensions | null): { extension: string; assistant: ExtensionAssistant } | null {
  const x = provided?.extensions.find((e) => e.assistant);
  return x?.assistant ? { extension: x.id, assistant: x.assistant } : null;
}
