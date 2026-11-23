// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The shell's commands, as the palette lists them. Shortcuts come from the one table in
// ./shortcuts; a command without a row there has no shortcut.

import { canFormat } from "../editor/format-body";
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
  "new-request": "New…",
  send: "Send request",
  save: "Save",
  "close-tab": "Close tab",
  palette: "Command palette",
  "toggle-theme": "Toggle theme",
  "focus-sidebar": "Focus sidebar",
  import: "Import collection…",
  clone: "Clone a repository…",
  search: "Search in collection",
  history: "Show history",
  git: "Show Git",
  plugins: "Manage plugins…",
  cookies: "Show cookies…",
  "format-body": "Format body",
  "go-to-tab": "Go to tab…",
  "next-tab": "Next tab",
  "previous-tab": "Previous tab",
  "tab-1": "Go to tab 1–9 (9: last)",
  "tab-2": "Go to tab 2",
  "tab-3": "Go to tab 3",
  "tab-4": "Go to tab 4",
  "tab-5": "Go to tab 5",
  "tab-6": "Go to tab 6",
  "tab-7": "Go to tab 7",
  "tab-8": "Go to tab 8",
  "tab-9": "Go to last tab",
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
    cmd("new-request"),
    ...(app.active ? [cmd("send")] : []),
    ...(app.active && canFormat(app.drafts[app.active]?.body) ? [cmd("format-body")] : []),
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
    cmd("clone"),
    ...(open ? [cmd("search"), cmd("history"), cmd("git")] : []),
    cmd("plugins"),
    ...(open ? [cmd("cookies")] : []),
    cmd("toggle-theme"),
    ...(app.active ? [cmd("close-tab")] : []),
    ...(app.openTabs.length > 0 ? [cmd("go-to-tab")] : []),
    ...(app.openTabs.length > 1 ? [cmd("next-tab"), cmd("previous-tab")] : []),
    ...(open ? [cmd("focus-sidebar")] : []),
  ];
}
