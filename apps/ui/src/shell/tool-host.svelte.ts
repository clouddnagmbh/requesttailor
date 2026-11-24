// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: main-area views of the new tools (comparing environments, lint, the webhook
// inbox, the plugin catalogue). One tool at a time; each strand exports its `ToolDefinition` from
// `<part>/tool.ts` and this file lists them. `AppState.show({ kind: "tool", tool })` opens one;
// a tool without a definition does nothing, so the app looks as before until a strand fills it.

import type { Component } from "svelte";

import { catalogueTool } from "../catalogue/tool";
import { envcompareTool } from "../envcompare/tool";
import { inboxTool } from "../inbox/tool";
import { lintTool } from "../lint/tool";
import type { AppState } from "../state/app.svelte";

export type ToolId = "env-compare" | "lint" | "inbox" | "catalogue";

export interface ToolProps {
  app: AppState;
  /** Leaves the tool and shows the editor again. */
  close(): void;
}

export interface ToolDefinition {
  id: ToolId;
  title: string;
  component: Component<ToolProps>;
}

/** The tools the build has; strands add theirs by exporting a definition in `<part>/tool.ts`. */
export function allTools(): ToolDefinition[] {
  return [envcompareTool, lintTool, inboxTool, catalogueTool].filter((t): t is ToolDefinition => t !== null);
}

export function toolFor(id: ToolId): ToolDefinition | undefined {
  return allTools().find((t) => t.id === id);
}

/** The tool open in the main area (`null`: none). */
export class Tools {
  current = $state<ToolDefinition | null>(null);

  /** Opens the tool; `false` (and nothing changes) if this build has no such tool. */
  open(id: ToolId): boolean {
    const tool = toolFor(id);
    if (!tool) return false;
    this.current = tool;
    return true;
  }

  close(): void {
    this.current = null;
  }
}
