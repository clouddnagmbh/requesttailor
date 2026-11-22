// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Labels for the semantic diff (wave 12, strand A).

import type { ChangeKind, ChangeTarget, DiffValue, FieldPath } from "../commands/types";

/** Last path segment of a secret variable that has a value written into its file. */
export const FILE_VALUE = "@file-value";

/** Whether the change is a value written into the file of a secret variable (never shown). */
export const isFileValue = (path: FieldPath): boolean => path[path.length - 1] === FILE_VALUE;

/** What a hidden value means at this field, in words. */
export function secretNote(path: FieldPath, old: DiffValue | null, now: DiffValue | null): string {
  if (isFileValue(path)) {
    const name = path[path.length - 2] ?? "";
    const verb = old && now ? "changed" : now ? "added" : "removed";
    return `A value that looks like a secret was ${verb} for "${name}" in this environment file. The value is hidden: secrets belong in the keychain, not in the file.`;
  }
  return old && now ? "changed" : now ? "added" : "removed";
}

const UNIQUE_SUFFIX = /#\d+$/;

function asRecord(v: unknown): Record<string, unknown> {
  return v && typeof v === "object" ? (v as Record<string, unknown>) : {};
}

/**
 * Like `valueText`, but assertions and actions read as text (`status eq 200`,
 * `set variable token from jsonq res.body.token`) instead of JSON.
 */
export function fieldValueText(path: FieldPath, v: DiffValue | null): string | null {
  if (!v || v.kind !== "json") return valueText(v);
  const kind = path[0] === "defaults" ? path[1] : path[0];
  const o = asRecord(v.value);
  const str = (x: unknown) => (typeof x === "string" ? x : x === undefined || x === null ? "" : String(x));
  const off = o.enabled === false ? " (off)" : "";
  if (kind === "assertions") {
    const expression = (path[path.length - 1] ?? "").replace(UNIQUE_SUFFIX, "");
    const value = str(o.value);
    return [expression, str(o.operator), value].filter((p) => p !== "").join(" ") + off;
  }
  if (kind === "actions") {
    const variable = (path[path.length - 1] ?? "").replace(UNIQUE_SUFFIX, "");
    const scope = str(o.scope);
    const from = [str(o.method), str(o.expression)].filter((p) => p !== "").join(" ");
    return `set ${scope ? scope + " " : ""}variable ${variable}${from ? " from " + from : ""}${off}`;
  }
  return valueText(v);
}

/** `["headers", "Authorization", "@enabled"]` → `headers › Authorization › enabled`. */
export function fieldLabel(path: FieldPath): string {
  return path
    .map((s) => (s === "@enabled" ? "enabled" : s === "@secret" ? "secret" : s === FILE_VALUE ? "value in the file" : s))
    .join(" › ");
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
