// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The shell's commands, as the palette lists them. Shortcuts come from the one table in
// ./shortcuts; a command without a row there has no shortcut.

import type { AppState } from "../state/app.svelte";
import { formatShortcut, shortcutFor, type CommandId } from "./shortcuts";

export interface Command {
  /** Unique; `CommandId` or `env:<name>`. */
  id: string;
  label: string;
  /** Display form of the shortcut, if any. */
  shortcut?: string;
  /** Marks the current choice (active environment). */
  checked?: boolean;
  run: () => unknown;
}

export type Actions = Record<CommandId, () => unknown>;

export const LABELS: Record<CommandId, string> = {
  "open-folder": "Open folder",
  "new-request": "New request",
  send: "Send request",
  save: "Save",
  "close-tab": "Close tab",
  palette: "Command palette",
  "toggle-theme": "Toggle theme",
  "focus-sidebar": "Focus sidebar",
  import: "Import collection…",
  search: "Search in collection",
  history: "Show history",
  git: "Show Git",
};

/** Commands available in the current state. */
export function buildCommands(app: AppState, actions: Actions, mac: boolean): Command[] {
  const cmd = (id: CommandId): Command => {
    const s = shortcutFor(id);
    return { id, label: LABELS[id], shortcut: s && formatShortcut(s, mac), run: actions[id] };
  };
  const open = app.collection !== null;
  return [
    cmd("open-folder"),
    ...(open ? [cmd("new-request")] : []),
    ...(app.active ? [cmd("send")] : []),
    ...(app.dirty.length > 0 ? [cmd("save")] : []),
    ...app.environments.map(
      (e): Command => ({
        id: `env:${e.name}`,
        label: `Environment: ${e.name}`,
        checked: e.name === app.environment,
        run: () => app.setEnvironment(e.name),
      }),
    ),
    cmd("import"),
    ...(open ? [cmd("search"), cmd("history"), cmd("git")] : []),
    cmd("toggle-theme"),
    ...(app.active ? [cmd("close-tab")] : []),
    ...(open ? [cmd("focus-sidebar")] : []),
  ];
}
