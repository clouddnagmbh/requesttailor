// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Labels for the semantic diff (wave 12, strand A).

import type { ChangeKind, ChangeTarget, DiffValue, FieldPath } from "../commands/types";

/** `["headers", "Authorization", "@enabled"]` → `headers › Authorization › enabled`. */
export function fieldLabel(path: FieldPath): string {
  return path.map((s) => (s === "@enabled" ? "enabled" : s === "@secret" ? "secret" : s)).join(" › ");
}

export const KIND_LABEL: Record<ChangeKind["kind"], string> = {
  added: "Added",
  removed: "Removed",
  modified: "Modified",
  moved: "Moved",
};

export function targetLabel(t: ChangeTarget): string {
  switch (t.kind) {
    case "collection":
      return "Collection";
    case "folder":
      return "Folder";
    case "request":
      return "Request";
    case "environment":
      return "Environment";
  }
}

/** The value as one string (text as is, JSON compact); `null` for a secret. */
export function valueText(v: DiffValue | null): string | null {
  if (!v || v.kind === "secret") return null;
  return v.kind === "text" ? v.text : JSON.stringify(v.value);
}

/** A multi-line value on either side is shown as a code diff. */
export function isMultiline(oldValue: DiffValue | null, newValue: DiffValue | null): boolean {
  return [oldValue, newValue].some((v) => v?.kind === "text" && v.text.includes("\n"));
}

export function summary(changes: { change: ChangeKind }[]): string {
  const n = (k: ChangeKind["kind"]) => changes.filter((c) => c.change.kind === k).length;
  const parts = (["modified", "added", "removed", "moved"] as const)
    .map((k) => [k, n(k)] as const)
    .filter(([, c]) => c > 0)
    .map(([k, c]) => `${c} ${k}`);
  return parts.length ? parts.join(", ") : "no changes";
}
