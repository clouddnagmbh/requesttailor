// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Hints and node actions of a distribution's extensions (`AppExtension.hints` and
// `nodeActions`, wave 14, strand C): collected here for the request editor (./ExtensionHints),
// the collection nodes and the context menus of the sidebar (strand A). Dismissals per device
// (strand C).

import type { ExtensionHint, ExtensionNodeAction, HintTarget, NodeTarget } from "../extensions";
import type { ProvidedExtensions } from "./extension-tabs";

export interface ExtensionHintRef {
  /** `<extension id>/<hint id>`, the key of its dismissal. */
  key: string;
  extension: string;
  hint: ExtensionHint;
}

export interface ExtensionNodeActionRef {
  /** `<extension id>/<action id>`. */
  key: string;
  extension: string;
  action: ExtensionNodeAction;
}

/** Every hint of `provided` for `target`, in extension order (dismissed ones included);
 *  an extension whose `hints` throws adds none. */
export function extensionHintsFor(provided: ProvidedExtensions | null, target: HintTarget): ExtensionHintRef[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => {
    try {
      return (x.hints?.(provided.ctx, target) ?? []).map((hint) => ({ key: `${x.id}/${hint.id}`, extension: x.id, hint }));
    } catch (e) {
      console.warn(`RequestTailor: hints of extension ${x.id} failed:`, e);
      return [];
    }
  });
}

/** The context-menu entries of `provided` for a collection or folder node. */
export function extensionNodeActions(provided: ProvidedExtensions | null, target: NodeTarget): ExtensionNodeActionRef[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => {
    try {
      return (x.nodeActions?.(provided.ctx, target) ?? []).map((action) => ({ key: `${x.id}/${action.id}`, extension: x.id, action }));
    } catch (e) {
      console.warn(`RequestTailor: node actions of extension ${x.id} failed:`, e);
      return [];
    }
  });
}
