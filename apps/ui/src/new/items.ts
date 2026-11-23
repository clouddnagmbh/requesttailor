// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tiles of a distribution's extensions in the "New…" dialog (`AppExtension.newItems`, wave 14).

import type { ExtensionNewItem } from "../extensions";
import type { ProvidedExtensions } from "../shell/extension-tabs";

export interface ExtensionNewItemRef {
  /** `<extension id>/<item id>`, unique; also `AppState.openNew({ kind })`. */
  key: string;
  extension: string;
  item: ExtensionNewItem;
}

/** The tiles of `provided`, in extension order; an extension whose `newItems` throws adds none. */
export function extensionNewItems(provided: ProvidedExtensions | null): ExtensionNewItemRef[] {
  if (!provided) return [];
  return provided.extensions.flatMap((x) => {
    try {
      return (x.newItems?.(provided.ctx) ?? []).map((item) => ({ key: `${x.id}/${item.id}`, extension: x.id, item }));
    } catch (e) {
      console.warn(`RequestTailor: new items of extension ${x.id} failed:`, e);
      return [];
    }
  });
}
