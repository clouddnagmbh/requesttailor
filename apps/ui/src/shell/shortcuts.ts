// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { formatShortcut as kitFormat, isMacPlatform, matchShortcut as kitMatch, type Shortcut as KitShortcut } from "@tailor-kit/shell";

// The one table of keyboard shortcuts. The global handler, the command palette and the
// start screen all read it. "Mod" is Cmd on macOS and Ctrl elsewhere. Every shortcut also
// fires while focus is in an input or a code editor (the handler listens in the capture
// phase, before the editor sees the key). Matching and display are @tailor-kit/shell.

export type CommandId =
  | "open-folder"
  | "new-request"
  | "send"
  | "save"
  | "close-tab"
  | "palette"
  | "toggle-theme"
  | "focus-sidebar"
  | "import"
  | "clone"
  | "search"
  | "history"
  | "git"
  | "plugins"
  | "cookies"
  | "format-body"
  | "go-to-tab"
  | "next-tab"
  | "previous-tab"
  | "group-new"
  | "group-toggle"
  | "group-close"
  | TabNumber;

/** `tab-1` … `tab-8` jump to that tab, `tab-9` to the last one (the browser convention). */
export type TabNumber = `tab-${1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9}`;
export const TAB_NUMBERS: readonly TabNumber[] = ["tab-1", "tab-2", "tab-3", "tab-4", "tab-5", "tab-6", "tab-7", "tab-8", "tab-9"];

export type Shortcut = KitShortcut<CommandId>;

export const SHORTCUTS: readonly Shortcut[] = [
  { command: "send", key: "Enter" },
  { command: "save", key: "s" },
  { command: "new-request", key: "n" },
  { command: "close-tab", key: "w" },
  { command: "open-folder", key: "o" },
  { command: "palette", key: "k" },
  { command: "format-body", key: "f", shiftAlt: true },
  { command: "go-to-tab", key: "p" },
  { command: "next-tab", key: "Tab", ctrl: true },
  { command: "previous-tab", key: "Tab", ctrl: true, shift: true },
  ...TAB_NUMBERS.map((command, i): Shortcut => ({ command, key: String(i + 1), unlisted: i > 0, digits: i === 0 })),
];

/** Further keys for a command that already has a row above (not listed, not shown in menus). */
export const ALIASES: readonly Shortcut[] = [
  { command: "next-tab", key: "]", code: "BracketRight", shift: true, macOnly: true },
  { command: "previous-tab", key: "[", code: "BracketLeft", shift: true, macOnly: true },
];

export { isMacPlatform };

type KeyLike = Parameters<typeof kitMatch>[0];

/** The shortcut this key event triggers, if any. */
export function matchShortcut(e: KeyLike, mac: boolean): Shortcut | undefined {
  return kitMatch(e, mac, SHORTCUTS, ALIASES);
}

export function shortcutFor(command: CommandId): Shortcut | undefined {
  return SHORTCUTS.find((s) => s.command === command);
}

/** Display form: "⌘S" / "⇧⌥F" on macOS, "Ctrl+S" / "Shift+Alt+F" elsewhere. */
export const formatShortcut = kitFormat;
