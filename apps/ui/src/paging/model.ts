// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand C: the words and the form state of paging. The rules and limits themselves live
// in Rust (`rt_app::paging`); here are only the labels, the form draft and what a finished run
// says about itself.

import type {
  CursorPlace,
  EffectivePaging,
  PagedResult,
  PagingLimits,
  PagingRule,
  PagingSpec,
  PagingStop,
} from "../commands/types";

/** The defaults of `rt_app::paging` (the core cuts anything above its hard maxima). */
export const DEFAULT_MAX_PAGES = 20;
export const HARD_MAX_PAGES = 1000;

export type RuleKind = PagingRule["kind"];

export const RULE_KINDS: { value: RuleKind; label: string; hint: string }[] = [
  { value: "link-header", label: "Link header", hint: 'The response has Link: <url>; rel="next".' },
  { value: "json-url", label: "URL in the body", hint: "A JSONPath to the next page's URL, absolute or relative." },
  { value: "cursor", label: "Cursor", hint: "A JSONPath to a cursor that goes back in the next request." },
];

export const CURSOR_PLACES: { value: CursorPlace; label: string }[] = [
  { value: "query", label: "Query parameter" },
  { value: "header", label: "Header" },
  { value: "body", label: "Body field" },
];

/** The form of the rule dialog. */
export interface RuleDraft {
  kind: RuleKind;
  next: string;
  place: CursorPlace;
  name: string;
  merge: string;
}

export function emptyDraft(): RuleDraft {
  return { kind: "link-header", next: "$.next", place: "query", name: "cursor", merge: "" };
}

export function draftOf(spec: PagingSpec | null): RuleDraft {
  const draft = emptyDraft();
  if (!spec) return draft;
  draft.kind = spec.rule.kind;
  if (spec.rule.kind !== "link-header") draft.next = spec.rule.next;
  if (spec.rule.kind === "cursor") {
    draft.place = spec.rule.param.place;
    draft.name = spec.rule.param.name;
  }
  draft.merge = spec.merge ?? "";
  return draft;
}

export function specOf(draft: RuleDraft): PagingSpec {
  const merge = draft.merge.trim();
  const spec: PagingSpec = { rule: { kind: "link-header" } };
  if (draft.kind === "json-url") spec.rule = { kind: "json-url", next: draft.next.trim() };
  if (draft.kind === "cursor") {
    spec.rule = { kind: "cursor", next: draft.next.trim(), param: { place: draft.place, name: draft.name.trim() } };
  }
  if (merge) spec.merge = merge;
  return spec;
}

/** What is missing in the draft, or `null` when it can be saved. */
export function draftProblem(draft: RuleDraft): string | null {
  if (draft.kind !== "link-header" && !draft.next.trim().startsWith("$")) return "The path to the next page starts with $, e.g. $.next";
  if (draft.kind === "cursor" && draft.name.trim() === "") return "Name the parameter the cursor goes in.";
  if (draft.merge.trim() !== "" && !draft.merge.trim().startsWith("$")) return "The merge path starts with $, e.g. $.items";
  return null;
}

export function ruleSummary(spec: PagingSpec): string {
  const r = spec.rule;
  const how =
    r.kind === "link-header"
      ? "Link header"
      : r.kind === "json-url"
        ? `URL at ${r.next}`
        : `cursor at ${r.next} as ${r.param.place} ${r.param.name}`;
  return spec.merge ? `${how}, items at ${spec.merge}` : how;
}

/** Where the rule comes from, for the chip: a plugin's rule is only offered. */
export function originText(found: EffectivePaging): string {
  switch (found.origin.kind) {
    case "request":
      return "Rule of this request";
    case "collection":
      return "Rule of the collection";
    case "plugin":
      return `Next page found (by ${found.origin.plugin})`;
  }
}

/** Why a run stopped, in a sentence. */
export function stopText(stop: PagingStop): string {
  switch (stop.kind) {
    case "done":
      return "no further page";
    case "max-pages":
      return "page limit reached";
    case "max-bytes":
      return "size limit reached";
    case "max-seconds":
      return "time limit reached";
    case "loop":
      return "the next page was already loaded";
    case "other-origin":
      return stop.message;
    case "error-status":
      return stop.message === 0 ? "a page got no answer" : `a page answered ${stop.message}`;
    case "refused":
      return `a page was refused: ${stop.message}`;
    case "not-read":
      return "the request writes";
  }
}

export function resultSummary(result: PagedResult): string {
  const pages = `${result.pages} ${result.pages === 1 ? "page" : "pages"}`;
  if (result.stopped.kind === "done" && !result.more) return pages;
  return `${pages}, stopped: ${stopText(result.stopped)}${result.more ? "; more pages exist" : ""}`;
}

export function limitsOf(maxPages: number): PagingLimits {
  const pages = Math.max(1, Math.min(HARD_MAX_PAGES, Math.floor(maxPages) || DEFAULT_MAX_PAGES));
  // Bytes and seconds: the core's defaults (50 MiB, 120 s).
  return { max_pages: pages, max_bytes: 50 * 1024 * 1024, max_seconds: 120 };
}

/** The last run per kept response: the chip tells what the page in view is. */
const runs = new Map<number, { pages: number; summary: string; more: boolean }>();

export function rememberRun(responseId: number, result: PagedResult, mode: "next" | "all"): void {
  runs.set(responseId, { pages: result.pages, summary: resultSummary(result), more: result.more && mode === "next" });
}

export function runOf(responseId: number | null): { pages: number; summary: string; more: boolean } | undefined {
  return responseId === null ? undefined : runs.get(responseId);
}
