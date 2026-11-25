// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The pure side of the access-check view (wave 19, strand D): sentences for a verdict, the tone
// it is shown in, and what is asked before it runs. Nothing here works out whether a request
// writes or whether an environment is production: the core answers (`agents.check`,
// `rt_app::access`), and the run itself refuses what must not run.

import type { AccessCheck, AccessEvidence, AccessVerdict } from "../commands/types";

export type VerdictTone = "ok" | "danger" | "warn" | "neutral";

export function tone(verdict: AccessVerdict): VerdictTone {
  switch (verdict.kind) {
    case "ok":
      return "ok";
    case "broken-object-access":
    case "broken-function-access":
      return "danger";
    case "inconclusive":
      return "warn";
    case "skipped":
      return "neutral";
  }
}

export function isBroken(verdict: AccessVerdict): boolean {
  return verdict.kind === "broken-object-access" || verdict.kind === "broken-function-access";
}

/** The one line over the result. */
export function headline(check: AccessCheck, a: string, b: string): string {
  switch (check.verdict.kind) {
    case "ok":
      return `${b} was denied: access looks right`;
    case "broken-object-access":
      return `${b} can read what belongs to ${a}`;
    case "broken-function-access":
      return `${b} can call what must be denied`;
    case "inconclusive":
      return "Inconclusive: the answers say neither yes nor no";
    case "skipped":
      return `Not checked: ${check.verdict.reason}`;
  }
}

/** A hint under a skipped check, when there is something the user can do. */
export function skippedHint(reason: string): string | null {
  if (reason === "skipped (writes)") return "This request writes. Tick “Allow writes for this check” to send it, in an environment that is not production.";
  if (reason === "skipped (production)") return "A writing request is never checked against a production environment.";
  if (reason === "no object reference found") return "The request needs an id: use a variable such as {{orderId}} in the path or query, so user B can be given the id user A saw.";
  return null;
}

/** The sentence for the question before writing requests are sent. */
export const WRITE_QUESTION = "Allow writes for this check?";

/** Header names, joined for one line. */
export function headerLine(e: AccessEvidence): string {
  return e.header_names.join(", ");
}

/** What is shown of one piece of evidence's values: `path = value`, at most `limit`. */
export function valueRows(e: AccessEvidence, limit = 12): { rows: [string, string][]; more: number } {
  return { rows: e.values.slice(0, limit), more: Math.max(0, e.values.length - limit) };
}

/** Environments user B can be, given user A. */
export function otherEnvironments(names: string[], a: string): string[] {
  return names.filter((n) => n !== a);
}
