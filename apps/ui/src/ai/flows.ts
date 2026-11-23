// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The entries of the app's own assistant tasks (wave 16, strand B), one function each: ask
// (./task.ts), then show or propose. Changes go through `suggest` (strand C): a diff the user
// applies or discards, unsaved, one undo step where the change can be undone.

import type { AiNameTarget, AiAnswer, RequestSpec } from "../commands/types";
import { createSuggest } from "../suggest/suggest";
import type { AppState } from "../state/app.svelte";
import type { AiState } from "./ai.svelte";
import { withSuggestedTests } from "./apply";
import DocsResult from "./DocsResult.svelte";
import NamesResult from "./NamesResult.svelte";
import { tryTask } from "./task";
import TextResult from "./TextResult.svelte";

async function currentSpec(app: AppState, path: string): Promise<RequestSpec> {
  await app.settled();
  const spec = app.drafts[path] ?? (await app.backend.getRequest(path));
  return JSON.parse(JSON.stringify(spec)) as RequestSpec;
}

/** "Explain" on a response that did not fail: a short summary of what came back. */
export async function explainResponse(app: AppState, ai: AiState, path: string, response: number): Promise<void> {
  const answer = await tryTask(app, ai, { task: "explain-response", path, response });
  if (!answer) return;
  await app.showDialog({ title: "About this response", component: TextResult, props: { text: answer.text, path }, width: 640 });
}

/** "Suggest from response": assertions for the current response, as a diff on the request. */
export async function suggestAssertions(app: AppState, ai: AiState, path: string, response: number): Promise<boolean> {
  const answer = await tryTask(app, ai, { task: "suggest-assertions", path, response });
  if (!answer) return false;
  const base = await currentSpec(app, path);
  const next = withSuggestedTests(base, { text: "", assertions: answer.assertions ?? [] });
  if (next.assertions.length === base.assertions.length) {
    app.error = "The request already has these assertions; nothing to add.";
    return false;
  }
  return createSuggest(app.backend, app)({
    kind: "request",
    path,
    spec: next,
    note: { title: "Assertions from the response", explanation: answer.text },
  });
}

/** A new request from a description or a snippet, as a "new request" suggestion. `false`
 *  when nothing was proposed (cancelled, failed or discarded) so the dialog can open again. */
export async function createFromText(
  app: AppState,
  ai: AiState,
  input: { mode: "describe" | "snippet"; text: string; folder: string },
): Promise<boolean> {
  const text = input.text.trim();
  const answer: AiAnswer | null = await tryTask(
    app,
    ai,
    input.mode === "snippet"
      ? { task: "request-from-snippet", snippet: text, folder: input.folder }
      : { task: "create-request", description: text, folder: input.folder },
  );
  if (!answer?.request) return false;
  return createSuggest(app.backend, app)({
    kind: "new-request",
    folder: input.folder,
    spec: answer.request,
    note: { title: "New request", explanation: answer.text },
  });
}

/** Names for a request, a folder or a variable. A request's name can be applied (a diff, one
 *  undo step); folders and variables have no rename in the app yet, so their proposals can
 *  be copied. */
export async function suggestNames(app: AppState, ai: AiState, target: AiNameTarget, path: string): Promise<void> {
  const answer = await tryTask(app, ai, { task: "suggest-names", target, path });
  if (!answer?.names?.length) return;
  const chosen = (await app.showDialog({
    title: target === "request" ? "Name this request" : target === "folder" ? "Name this folder" : "Name the variables",
    component: NamesResult,
    props: { names: answer.names, target, path },
    width: 520,
  })) as { name?: string } | undefined;
  if (target !== "request" || !chosen?.name) return;
  const spec = await currentSpec(app, path);
  await createSuggest(app.backend, app)({ kind: "request", path, spec: { ...spec, name: chosen.name } });
}

/** A description draft for a request. Shown as Markdown; it can be saved as a Markdown file
 *  next to the request (the request model has no description field yet), as a diff. */
export async function draftDocs(app: AppState, ai: AiState, path: string): Promise<void> {
  const answer = await tryTask(app, ai, { task: "draft-docs", path });
  if (!answer?.docs) return;
  const file = path.replace(/\.yml$/, "") + ".md";
  const result = (await app.showDialog({
    title: "Draft description",
    component: DocsResult,
    props: { docs: answer.docs, path, file },
    width: 640,
  })) as { save?: boolean } | undefined;
  if (!result?.save) return;
  await createSuggest(app.backend, app)({
    kind: "file",
    path: file,
    text: answer.docs.endsWith("\n") ? answer.docs : `${answer.docs}\n`,
    note: { title: "Description of " + path },
  });
}
