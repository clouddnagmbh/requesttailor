// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Matches a CodeMirror-style key (`Mod-i`, `Mod-Shift-Enter`, `Alt-s`) against a keyboard event,
// for the shortcuts of `ExtensionEditorAction.shortcut`. `Mod` is Cmd on macOS, Ctrl elsewhere.

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey">;

export function matchesKey(spec: string, e: KeyLike, mac: boolean): boolean {
  const parts = spec.split(/[-+]/).filter(Boolean);
  const key = parts.pop();
  if (!key) return false;
  const mods = new Set(parts.map((p) => p.toLowerCase()));
  const mod = mods.has("mod") || (mac ? mods.has("cmd") || mods.has("meta") : mods.has("ctrl"));
  const wantCtrl = mods.has("ctrl") && mac;
  const wantMeta = mac ? mod : false;
  const wantControl = mac ? wantCtrl : mod;
  if (e.metaKey !== wantMeta || e.ctrlKey !== wantControl) return false;
  if (e.altKey !== mods.has("alt")) return false;
  // Shift is part of the key for letters ("I"), so only a named Shift counts for those.
  if (mods.has("shift") !== e.shiftKey) return false;
  return e.key.toLowerCase() === key.toLowerCase();
}
