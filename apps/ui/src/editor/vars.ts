// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Variable placeholders and their provenance for highlighting. The editor never resolves a
// variable itself: states and values come from the core's `preview`.

import type { Preview, VarSource } from "../commands/types";

export interface Placeholder {
  from: number;
  to: number;
  name: string;
}

/** `{{name}}` occurrences, same matches as rt-core and Bruno (`\{\{([^}]+)\}\}`). */
export function findPlaceholders(text: string): Placeholder[] {
  const out: Placeholder[] = [];
  const re = /\{\{([^}]+)\}\}/g;
  for (let m = re.exec(text); m; m = re.exec(text)) {
    out.push({ from: m.index, to: m.index + m[0].length, name: m[1] });
  }
  return out;
}

export type VarState = "resolved" | "secret" | "unresolved" | "unknown";

export interface VarInfo {
  state: VarState;
  /** Where the value comes from; `null` if unresolved or not in the preview. */
  source: VarSource | null;
  /** Resolved value; always `null` for secrets. */
  value: string | null;
}

/** Name → info. Names the preview does not mention (not sent, or not loaded yet) are absent. */
export type VarLookup = ReadonlyMap<string, VarInfo>;

export function lookupFromPreview(preview: Preview | null): VarLookup {
  const map = new Map<string, VarInfo>();
  if (!preview) return map;
  for (const name of preview.unresolved) map.set(name, { state: "unresolved", source: null, value: null });
  for (const v of preview.variables) {
    map.set(v.name, {
      state: v.secret ? "secret" : "resolved",
      source: v.source,
      value: v.secret ? null : v.value,
    });
  }
  return map;
}

export function infoFor(lookup: VarLookup, name: string): VarInfo {
  return lookup.get(name) ?? { state: "unknown", source: null, value: null };
}

export function sourceLabel(source: VarSource): string {
  switch (source.layer) {
    case "environment":
      return `environment ${source.name}`;
    case "folder":
      return `folder ${source.path}`;
    default:
      return source.layer;
  }
}

/** Tooltip lines for a placeholder: title and detail. Never contains a secret value. */
export function describe(name: string, info: VarInfo): { title: string; detail: string } {
  switch (info.state) {
    case "resolved":
      return {
        title: `${name} · ${sourceLabel(info.source!)}`,
        detail: info.value === null ? "(no value)" : info.value === "" ? "(empty)" : info.value,
      };
    case "secret":
      return { title: `${name} · ${info.source ? sourceLabel(info.source) : "secret"}`, detail: "secret" };
    case "unresolved":
      return { title: `${name} · unresolved`, detail: "Not defined in any layer for this environment" };
    case "unknown":
      return { title: name, detail: "Not part of what would be sent" };
  }
}
