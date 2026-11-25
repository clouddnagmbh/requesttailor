// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: a counter the views tab reads, bumped when a view is saved or removed
// anywhere (the "Save as view" dialog, the tab's "Remove"), so every open tab asks again.

let version = $state(0);

/** Changes whenever the saved views of some request may have changed. */
export function viewsVersion(): number {
  return version;
}

export function bumpViews(): void {
  version++;
}
