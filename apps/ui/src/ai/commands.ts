// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the AI assistant (wave 8, strand E). Settings are always there (that is
// how it is turned on); the tasks only once it is set up and they apply.

import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";
import type { AiState } from "./ai.svelte";
import { failed, folderOf } from "./apply";
import { draftDocs, explainResponse, suggestAssertions, suggestNames } from "./flows";

export function aiCommands(app: AppState, ai: AiState): Command[] {
  const out: Command[] = [
    { id: "ai-settings", label: "AI assistant: settings…", run: () => ai.open({ kind: "settings" }) },
  ];
  // Nothing else is offered until the assistant is set up; where the collection turned it off
  // the entries are not there either (the buttons say "AI is off for this collection").
  if (!ai.ready || !app.collection) return out;
  out.push({
    id: "ai-create",
    label: "AI: new request from a description or snippet…",
    run: () => ai.open({ kind: "create", folder: folderOf(app.active) }),
  });
  const path = app.active;
  const response = path ? app.results[path]?.response_id : null;
  if (path && response != null) {
    out.push(
      {
        id: "ai-explain",
        label: "AI: explain this response",
        // A failed response is explained as an error, any other is summarised.
        run: () => (failed(app.results[path]) ? ai.open({ kind: "explain", path, response }) : void explainResponse(app, ai, path, response)),
      },
      { id: "ai-tests", label: "AI: suggest tests for this request", run: () => ai.open({ kind: "tests", path, response }) },
      { id: "ai-assertions", label: "AI: suggest assertions from the response", run: () => void suggestAssertions(app, ai, path, response) },
    );
  }
  if (path) {
    out.push(
      { id: "ai-name-request", label: "AI: suggest a name for this request", run: () => void suggestNames(app, ai, "request", path) },
      { id: "ai-name-variables", label: "AI: suggest variable names for this request", run: () => void suggestNames(app, ai, "variable", path) },
      { id: "ai-docs", label: "AI: draft a description for this request", run: () => void draftDocs(app, ai, path) },
    );
  }
  const folder = folderOf(path);
  if (folder) {
    out.push({ id: "ai-name-folder", label: "AI: suggest a name for this folder", run: () => void suggestNames(app, ai, "folder", folder) });
  }
  return out;
}
