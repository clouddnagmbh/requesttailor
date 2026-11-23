// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Running one of the app's own assistant tasks (wave 16, strand B), the same way every time:
//
// 1. The collection must allow the assistant (`policy`); if not, nothing is built.
// 2. A cURL snippet is read by the core's importer, without a model: no preview, no call.
// 3. Otherwise the exact, masked call is previewed; the user confirms it when the settings ask
//    for that (first use of a profile, or always), and only then does it leave.
// 4. The answer is a proposal. Nothing is saved here.
//
// Nothing in this file runs until the user clicks an entry; when the assistant is not set up
// the entries are not there.

import { BackendError } from "../commands/backend";
import type { AiAnswer, AiTask } from "../commands/types";
import type { AppState } from "../state/app.svelte";
import type { AiState } from "./ai.svelte";
import { askError } from "./apply";
import { dialogConfirm } from "./extension-ai";

/** What an entry says when the collection turned the assistant off. */
export const OFF_MESSAGE = "AI is off for this collection";

/** Whether `snippet` is a `curl` command line (mirrors `rt_app::ai::snippet::is_curl`). */
export function looksLikeCurl(snippet: string): boolean {
  const text = snippet.trimStart().replace(/^\$ /, "").trimStart();
  return /^curl(\s|$)/.test(text);
}

/** Whether `task` needs no model (the core answers it from the importer). */
export function isLocal(task: AiTask): boolean {
  return task.task === "request-from-snippet" && looksLikeCurl(task.snippet);
}

/** Runs `task`; `null` if the user cancelled the preview. Rejects with the backend's error
 *  (`opted-out`, `not-configured`, `no-key`, a provider error, an unusable answer). */
export async function askTask(app: AppState, ai: AiState, task: AiTask, working = "Asking the model…"): Promise<AiAnswer | null> {
  const backend = app.backend;
  const input = { environment: app.environment, vars: [] };
  if (!isLocal(task)) {
    const policy = await backend.ai.policy();
    if (policy.file_off || policy.device_off) throw new BackendError("opted-out", OFF_MESSAGE);
    const preview = await backend.ai.preview({ kind: "task", task }, input);
    if (preview.required) {
      if (!(await dialogConfirm(app)(preview))) return null;
      await backend.ai.confirmPreview(preview.profile);
    }
  }
  ai.working = isLocal(task) ? "Reading the cURL command…" : working;
  try {
    return await backend.ai.ask(task, input);
  } finally {
    ai.working = null;
  }
}

/** `askTask` for entries: a failure goes to the app's error banner, `null` on any failure
 *  or cancel. */
export async function tryTask(app: AppState, ai: AiState, task: AiTask, working?: string): Promise<AiAnswer | null> {
  try {
    return await askTask(app, ai, task, working);
  } catch (e) {
    if (typeof e === "object" && e !== null && (e as { kind?: unknown }).kind === "cancelled") return null;
    app.error = askError(e);
    return null;
  }
}
