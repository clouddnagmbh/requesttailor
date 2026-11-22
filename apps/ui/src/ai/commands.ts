// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Palette entries of the AI assistant (wave 8, strand E). Settings are always there (that is
// how it is turned on); the tasks only once it is set up and they apply.

import type { Command } from "../shell/commands";
import type { AppState } from "../state/app.svelte";
import type { AiState } from "./ai.svelte";
import { folderOf } from "./apply";

export function aiCommands(app: AppState, ai: AiState): Command[] {
  const out: Command[] = [
    { id: "ai-settings", label: "AI assistant: settings…", run: () => ai.open({ kind: "settings" }) },
  ];
  if (!ai.configured || !app.collection) return out;
  out.push({
    id: "ai-create",
    label: "AI: create request from a description…",
    run: () => ai.open({ kind: "create", folder: folderOf(app.active) }),
  });
  const path = app.active;
  const response = path ? app.results[path]?.response_id : null;
  if (path && response != null) {
    out.push(
      { id: "ai-explain", label: "AI: explain this response", run: () => ai.open({ kind: "explain", path, response }) },
      { id: "ai-tests", label: "AI: suggest tests for this request", run: () => ai.open({ kind: "tests", path, response }) },
    );
  }
  return out;
}
