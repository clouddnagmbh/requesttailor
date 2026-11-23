// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The one table of keyboard shortcuts. The global handler, the command palette and the
// start screen all read it. "Mod" is Cmd on macOS and Ctrl elsewhere. Every shortcut also
// fires while focus is in an input or a code editor (the handler listens in the capture
// phase, before the editor sees the key).

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
  | "format-body";

export interface Shortcut {
  command: CommandId;
  /** `KeyboardEvent.key`, compared case-insensitively. Combined with Mod unless `shiftAlt`. */
  key: string;
  /** Shift+Alt instead of Mod (a letter key, matched by `KeyboardEvent.code` too: Alt changes `key` on macOS). */
  shiftAlt?: boolean;
}

export const SHORTCUTS: readonly Shortcut[] = [
  { command: "send", key: "Enter" },
  { command: "save", key: "s" },
  { command: "new-request", key: "n" },
  { command: "close-tab", key: "w" },
  { command: "open-folder", key: "o" },
  { command: "palette", key: "k" },
  { command: "format-body", key: "f", shiftAlt: true },
];

export function isMacPlatform(nav: { platform?: string; userAgent?: string } | undefined = globalThis.navigator): boolean {
  const p = nav?.platform || nav?.userAgent || "";
  return /Mac|iPhone|iPad|iPod/.test(p);
}

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"> & { code?: string };

/** The shortcut this key event triggers, if any. */
export function matchShortcut(e: KeyLike, mac: boolean): Shortcut | undefined {
  const key = e.key.toLowerCase();
  if (e.shiftKey && e.altKey && !e.metaKey && !e.ctrlKey) {
    return SHORTCUTS.find((s) => s.shiftAlt && (e.code === `Key${s.key.toUpperCase()}` || s.key.toLowerCase() === key));
  }
  const mod = mac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!mod || e.altKey || e.shiftKey) return undefined;
  return SHORTCUTS.find((s) => !s.shiftAlt && s.key.toLowerCase() === key);
}

export function shortcutFor(command: CommandId): Shortcut | undefined {
  return SHORTCUTS.find((s) => s.command === command);
}

/** Display form: "⌘S" / "⇧⌥F" on macOS, "Ctrl+S" / "Shift+Alt+F" elsewhere. */
export function formatShortcut(s: Shortcut, mac: boolean): string {
  const key = s.key === "Enter" ? (mac ? "↩" : "Enter") : s.key.toUpperCase();
  if (s.shiftAlt) return mac ? `⇧⌥${key}` : `Shift+Alt+${key}`;
  return mac ? `⌘${key}` : `Ctrl+${key}`;
}
