// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the app does at start before the user does anything: reopen the last collection
// without a prompt where the platform allows it (budget: send a request in under three
// keystrokes from app start). Owner: wave 4, strand C.
//
// The shell paints first, then the last collection is loaded: nothing here delays the first
// frame. The desktop's cold-start measurement (perf/README.md) uses this order: the first
// backend call after the first frame, `recentCollections()`, ends the measured interval.

import type { CollectionInfo } from "../commands/backend";
import type { AppState } from "../state/app.svelte";

/** Resolves once the browser has rendered a frame and is back in its event loop (a task
 *  after the next animation frame). There is deliberately no timeout: a hidden window
 *  (minimized, covered, locked screen) renders no frames, and the collection opens when it
 *  becomes visible, so the measurement never mistakes a timer for a frame. Without
 *  `requestAnimationFrame` (unit tests) it resolves after a task. */
export function afterFirstFrame(): Promise<void> {
  return new Promise((resolve) => {
    const raf = globalThis.requestAnimationFrame;
    if (typeof raf !== "function") {
      setTimeout(resolve, 0);
      return;
    }
    raf(() => setTimeout(resolve, 0));
  });
}

/** The collection to show at start, opened by the backend, or `null` for the start screen.
 *  Only the most recent collection is tried, without a prompt (`interactive: false`); if the
 *  platform needs consent first, the start screen's recent list offers it with one click. */
export async function reopenOnStart(app: AppState): Promise<CollectionInfo | null> {
  await afterFirstFrame();
  const recent = await app.backend.recentCollections();
  const last = recent[0];
  if (!last || app.collection) return null;
  return app.backend.openRecent(last.id, false);
}
