// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries for sharing and copying (wave 17, strand C): share the active request as a
// link or QR code, copy it as curl, fetch or Python, and open a pasted link or cURL command.

import type { Command } from "../shell/commands";
import ShareDialog from "../shell/ShareDialog.svelte";
import type { AppState } from "../state/app.svelte";
import PasteLinkDialog from "./PasteLinkDialog.svelte";

/** Opens the dialog for pasting a link or a cURL command. */
export function openPasteLink(app: AppState): void {
  void app.showDialog({ title: "Paste link", component: PasteLinkDialog, width: 560 });
}

/** Opens the "Share" dialog for `path`, on the given tab. */
export function openShare(app: AppState, path: string, tab: "link" | "curl" | "fetch" | "python" = "link"): void {
  void app.showDialog({ title: "Share request", component: ShareDialog, props: { path, tab }, width: 640 });
}

export function shareCommands(app: AppState): Command[] {
  const out: Command[] = [{ id: "share-paste", label: "Paste link…", run: () => openPasteLink(app) }];
  const path = app.active;
  if (!path || !app.collection) return out;
  out.push(
    { id: "share-link", label: "Share: link and QR code…", run: () => openShare(app, path) },
    { id: "share-curl", label: "Copy as curl…", run: () => openShare(app, path, "curl") },
    { id: "share-fetch", label: "Copy as fetch (JavaScript)…", run: () => openShare(app, path, "fetch") },
    { id: "share-python", label: "Copy as Python (requests)…", run: () => openShare(app, path, "python") },
  );
  return out;
}
