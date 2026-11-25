// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: "Save as view" next to the filter of the response (`ResponsePanel`).

import type { AppState } from "../state/app.svelte";
import SaveViewDialog from "./SaveViewDialog.svelte";

/** Opens the dialog for the JSONPath `expression` of the request at `path`; resolves to the
 *  name of the saved view, or `null` if the dialog was cancelled. */
export async function saveAsView(app: AppState, path: string, expression: string): Promise<string | null> {
  const result = await app.showDialog({
    title: "Save as view",
    component: SaveViewDialog,
    width: 480,
    props: { path, expression: expression.trim() },
  });
  return typeof result === "string" ? result : null;
}
