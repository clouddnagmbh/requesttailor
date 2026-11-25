// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The pure side of the minimal-case view (wave 19, strand E): what the user picked becomes a
// `ShrinkPredicate`, and a `ShrinkResult` becomes sentences. Nothing here works out whether a
// request writes or is production: the core answers (`agents.check`, `rt_app::shrink`).

import type { ShrinkPredicate, ShrinkResult, ShrinkStep, StoredResponse } from "../commands/types";

export type PredicateChoice =
  | "last"
  | "status"
  | "server-error"
  | "assertion"
  | "body"
  | "header-present"
  | "header-missing";

export const PREDICATE_CHOICES: { value: PredicateChoice; label: string }[] = [
  { value: "last", label: "The failure of the last response" },
  { value: "status", label: "Status is …" },
  { value: "server-error", label: "Any 5xx status" },
  { value: "assertion", label: "A test or assertion fails" },
  { value: "body", label: "Body matches …" },
  { value: "header-present", label: "Response has the header …" },
  { value: "header-missing", label: "Response lacks the header …" },
];

/** The text field belongs to these choices. */
export function needsValue(choice: PredicateChoice): boolean {
  return choice === "status" || choice === "body" || choice === "header-present" || choice === "header-missing";
}

export function valueLabel(choice: PredicateChoice): string {
  switch (choice) {
    case "status":
      return "Status code";
    case "body":
      return "Regular expression";
    default:
      return "Header name";
  }
}

/** The predicate for the choice, or a sentence saying what is missing. */
export function buildPredicate(choice: PredicateChoice, value: string): ShrinkPredicate | string {
  const text = value.trim();
  switch (choice) {
    case "last":
      return { kind: "last-failure" };
    case "server-error":
      return { kind: "server-error" };
    case "assertion":
      return { kind: "assertion-fails" };
    case "status": {
      const status = Number(text);
      if (!Number.isInteger(status) || status < 100 || status > 599) return "Enter a status code between 100 and 599.";
      return { kind: "status", status };
    }
    case "body":
      if (text === "") return "Enter a regular expression for the body.";
      try {
        new RegExp(text);
      } catch {
        return "That regular expression is not valid.";
      }
      return { kind: "body-matches", pattern: text };
    case "header-present":
      return text === "" ? "Enter the name of the header." : { kind: "header-present", name: text };
    case "header-missing":
      return text === "" ? "Enter the name of the header." : { kind: "header-missing", name: text };
  }
}

/** The predicate in words. */
export function describePredicate(p: ShrinkPredicate): string {
  switch (p.kind) {
    case "status":
      return `status is ${p.status}`;
    case "server-error":
      return "any 5xx status";
    case "assertion-fails":
      return p.assertion === undefined ? "a test or assertion fails" : `assertion ${p.assertion} fails`;
    case "body-matches":
      return `body matches /${p.pattern}/`;
    case "header-present":
      return `response has the header ${p.name}`;
    case "header-missing":
      return `response lacks the header ${p.name}`;
    case "last-failure":
      return "the failure of the last response";
  }
}

/** Which choice and value show a predicate (the default the view starts with). */
export function choiceOf(p: ShrinkPredicate): { choice: PredicateChoice; value: string } {
  switch (p.kind) {
    case "status":
      return { choice: "status", value: String(p.status) };
    case "server-error":
      return { choice: "server-error", value: "" };
    case "assertion-fails":
      return { choice: "assertion", value: "" };
    case "body-matches":
      return { choice: "body", value: p.pattern };
    case "header-present":
      return { choice: "header-present", value: p.name };
    case "header-missing":
      return { choice: "header-missing", value: p.name };
    case "last-failure":
      return { choice: "last", value: "" };
  }
}

/** `header:Authorization, query:key` → `["header:Authorization", "query:key"]`. */
export function parseKeep(text: string): string[] {
  return text
    .split(/[\n,]/)
    .map((s) => s.trim())
    .filter((s) => s !== "");
}

/** Whether the last response of a request failed (a status from 400 or a failed test). */
export function failed(response: StoredResponse | undefined): boolean {
  const r = response?.report;
  if (!r) return false;
  if ((r.response?.status ?? 0) >= 400) return true;
  return (r.assertions ?? []).some((a) => !a.passed) || (r.tests ?? []).some((t) => !t.passed);
}

export const MAX_PROBES_DEFAULT = 100;
export const MAX_PROBES_HARD = 500;

/** "This sends up to N requests that write." */
export function writeQuestion(probes: number): string {
  return `This sends up to ${probes} requests that write.`;
}

export type Outcome = "found" | "stopped" | "flaky" | "not-reproduced";

export function outcomeOf(r: ShrinkResult): Outcome {
  if (r.not_reproduced) return "not-reproduced";
  if (r.flaky && r.removed.length === 0) return "flaky";
  return r.stopped_by ? "stopped" : "found";
}

const STOPPED: Record<string, string> = {
  probes: "the limit of probes was reached",
  time: "the time limit was reached",
  cancelled: "the search was cancelled",
  "rate-limited": "the server asked for a pause (429)",
  error: "a probe could not be sent",
};

export function stoppedText(reason: string): string {
  if (reason.startsWith("blocked: ")) return `a rule of this installation refused a probe (${reason.slice(9)})`;
  return STOPPED[reason] ?? reason;
}

/** The headline of a result. */
export function headline(r: ShrinkResult): string {
  switch (outcomeOf(r)) {
    case "not-reproduced":
      return "The failure did not show again, so there is nothing to shrink.";
    case "flaky":
      return "The failure comes and goes (flaky), so nothing was shrunk.";
    default: {
      const parts = r.removed.filter((s) => s.kind !== "value").length;
      const values = r.removed.length - parts;
      let text = `Removed ${parts} ${parts === 1 ? "part" : "parts"} in ${r.probes} ${r.probes === 1 ? "probe" : "probes"}`;
      if (values > 0) text += `, simplified ${values} ${values === 1 ? "value" : "values"}`;
      return `${text}.`;
    }
  }
}

/** Notes under the headline (flaky, stopped early, 1-minimal). */
export function notes(r: ShrinkResult): string[] {
  const out: string[] = [];
  if (r.flaky && r.removed.length > 0) out.push("Flaky: the failure came and went, so the result may not be minimal.");
  if (r.stopped_by) out.push(`Stopped early: ${stoppedText(r.stopped_by)}; the result may not be minimal.`);
  if (r.one_minimal) out.push("1-minimal: taking away any single part makes the failure go.");
  return out;
}

/** "What was removed", grouped by kind, in the order of the kinds below. */
const KIND_ORDER = ["header", "query", "cookie", "form", "body", "value"];
const KIND_TITLE: Record<string, string> = {
  header: "Headers",
  query: "Query parameters",
  cookie: "Cookies",
  form: "Form fields",
  body: "Body",
  value: "Shortened values",
};

export function removedGroups(r: ShrinkResult): { kind: string; title: string; steps: ShrinkStep[] }[] {
  const by = new Map<string, ShrinkStep[]>();
  for (const s of r.removed) by.set(s.kind, [...(by.get(s.kind) ?? []), s]);
  return [...by.entries()]
    .sort(([a], [b]) => (KIND_ORDER.indexOf(a) + 1 || 99) - (KIND_ORDER.indexOf(b) + 1 || 99))
    .map(([kind, steps]) => ({ kind, title: KIND_TITLE[kind] ?? kind, steps }));
}

/** A part id without its kind: `header:X-Debug` → `X-Debug`. */
export function partName(step: ShrinkStep): string {
  return step.part.includes(":") ? step.part.slice(step.part.indexOf(":") + 1) : step.part;
}
