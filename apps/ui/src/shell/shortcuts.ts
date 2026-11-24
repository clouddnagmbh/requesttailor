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

export interface Shortcut {
  command: CommandId;
  /** `KeyboardEvent.key`, compared case-insensitively. Combined with Mod unless `shiftAlt`. */
  key: string;
  /** Shift+Alt instead of Mod (a letter key, matched by `KeyboardEvent.code` too: Alt changes `key` on macOS). */
  shiftAlt?: boolean;
  /** The Control key itself on every platform (not Cmd on macOS): Ctrl+Tab. */
  ctrl?: boolean;
  /** Shift is part of the shortcut (with Mod or Ctrl). */
  shift?: boolean;
  /** Matched by `KeyboardEvent.code` (layout independent) instead of `key`. */
  code?: string;
  /** Only on macOS (Cmd+Shift+] is the macOS convention; elsewhere those keys are typing). */
  macOnly?: boolean;
  /** Not listed on the start screen (the digit shortcuts show as one row). */
  unlisted?: boolean;
}

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
  ...TAB_NUMBERS.map((command, i): Shortcut => ({ command, key: String(i + 1), unlisted: i > 0 })),
];

/** Further keys for a command that already has a row above (not listed, not shown in menus). */
export const ALIASES: readonly Shortcut[] = [
  { command: "next-tab", key: "]", code: "BracketRight", shift: true, macOnly: true },
  { command: "previous-tab", key: "[", code: "BracketLeft", shift: true, macOnly: true },
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
  const onlyCtrl = e.ctrlKey && !e.metaKey;
  if (e.altKey) return undefined;
  return [...SHORTCUTS, ...ALIASES].find((s) => {
    if (s.shiftAlt || (s.macOnly && !mac)) return false;
    if (!(s.ctrl ? onlyCtrl : mod) || e.shiftKey !== !!s.shift) return false;
    return s.code ? e.code === s.code || key === s.key : s.key.toLowerCase() === key;
  });
}

export function shortcutFor(command: CommandId): Shortcut | undefined {
  return SHORTCUTS.find((s) => s.command === command);
}

/** Display form: "⌘S" / "⇧⌥F" on macOS, "Ctrl+S" / "Shift+Alt+F" elsewhere. */
export function formatShortcut(s: Shortcut, mac: boolean): string {
  if (s.command === "tab-1") return mac ? "⌘1…9" : "Ctrl+1…9";
  if (s.ctrl) return `${mac ? "⌃" : "Ctrl+"}${s.shift ? (mac ? "⇧" : "Shift+") : ""}${s.key}`;
  const key = s.key === "Enter" ? (mac ? "↩" : "Enter") : s.key.toUpperCase();
  if (s.shiftAlt) return mac ? `⇧⌥${key}` : `Shift+Alt+${key}`;
  return mac ? `⌘${key}` : `Ctrl+${key}`;
}
