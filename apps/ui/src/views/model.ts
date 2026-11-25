// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: pure helpers of the saved views (names, columns, how a cell reads). The
// evaluation itself is the core's (`rt_app::views`); nothing here parses a body.

import type { ResponseView, ViewDisplay, ViewResult } from "../commands/types";

export const NAME_LIMIT = 80;
/** A cell shows at most this many characters. */
export const CELL_LIMIT = 200;

export const DISPLAYS: { value: ViewDisplay; label: string; hint: string }[] = [
  { value: "json", label: "JSON", hint: "The matches as JSON" },
  { value: "table", label: "Table", hint: "An array of objects as a table" },
  { value: "text", label: "Text", hint: "One match per line" },
];

/** Names the dialog accepts: 1 to 80 characters, no control characters. */
export function nameProblem(name: string): string | null {
  const text = name.trim();
  if (text === "") return "Give the view a name.";
  if ([...text].length > NAME_LIMIT) return `Use at most ${NAME_LIMIT} characters.`;
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001f\u007f]/.test(text)) return "A name cannot contain control characters.";
  return null;
}

/** True if saving `name` replaces a view that is already there. */
export function replaces(name: string, existing: ResponseView[]): boolean {
  return existing.some((v) => v.name === name.trim());
}

/** "id, name" → ["id", "name"]; empty parts dropped, duplicates kept once. */
export function splitColumns(text: string): string[] {
  const out: string[] = [];
  for (const part of text.split(",")) {
    const c = part.trim();
    if (c !== "" && !out.includes(c)) out.push(c);
  }
  return out;
}

/** How one value reads in a table cell or a text line. */
export function cellText(value: unknown): string {
  if (value === null || value === undefined) return "";
  const text = typeof value === "string" ? value : JSON.stringify(value);
  return text.length > CELL_LIMIT ? `${text.slice(0, CELL_LIMIT)}…` : text;
}

/** The value a JSON view shows: the single match itself, else the list of matches. */
export function jsonOf(result: ViewResult): unknown {
  return result.values.length === 1 ? result.values[0] : result.values;
}

/** The lines of a text view. */
export function textLines(result: ViewResult): string[] {
  return result.values.map((v) => (typeof v === "string" ? v : JSON.stringify(v)));
}

/** A short label for the view list: the name, and the language if it is not JSONPath. */
export function viewLabel(view: ResponseView): string {
  return view.language === "jq" ? `${view.name} (jq)` : view.name;
}

/** Whether a response is worth offering views for: a body that reads as JSON. */
export function looksJson(contentType: string | null, preview: string): boolean {
  if (contentType && /json/i.test(contentType)) return true;
  const head = preview.trimStart()[0];
  return head === "{" || head === "[";
}
