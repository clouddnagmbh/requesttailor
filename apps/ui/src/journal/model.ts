// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the test-data journal view (wave 19, strand C).

import type { JournalEntry, RevertOutcome, RevertPlan, StepResult, TreeNode } from "../commands/types";

/** `{ path, label }` of every request in the tree, in tree order. */
export function requestChoices(tree: TreeNode | null): { path: string; label: string }[] {
  const out: { path: string; label: string }[] = [];
  const walk = (node: TreeNode, trail: string[]) => {
    if (node.kind === "request") out.push({ path: node.path, label: [...trail, node.name].join(" / ") });
    else node.children.forEach((c) => walk(c, node.path === "" ? trail : [...trail, node.name]));
  };
  if (tree) walk(tree, []);
  return out;
}

/** The first thing to show for an entry, e.g. `POST /orders → 201`. */
export function entryTitle(e: JournalEntry): string {
  return `${e.method} ${e.url}`;
}

/** What the entry names, in a few words: the id, else the Location. */
export function entryTarget(e: JournalEntry): string | null {
  if (e.resource_id) return `id ${e.resource_id}`;
  if (e.location) return e.location;
  return null;
}

export function timeLabel(ms: number, now: number = Date.now()): string {
  const d = new Date(ms);
  const sameDay = new Date(now).toDateString() === d.toDateString();
  const pad = (n: number) => String(n).padStart(2, "0");
  const clock = `${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  return sameDay ? clock : `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${clock}`;
}

/** The question before reverting in production. */
export function productionQuestion(plan: RevertPlan): string {
  const n = plan.steps.filter((s) => s.state === "planned").length;
  return `Revert ${n} ${n === 1 ? "change" : "changes"} in PRODUCTION?`;
}

const RESULT_TEXT: Record<StepResult, string> = {
  reverted: "Reverted",
  "skipped-conflict": "Skipped: changed by someone else",
  "skipped-not-reversible": "Skipped: not reversible",
  failed: "Failed",
};

export function resultLine(result: StepResult, message: string): string {
  const head = RESULT_TEXT[result];
  return message && result !== "reverted" ? `${head}: ${message}` : message ? `${head} (${message})` : head;
}

export function outcomeTone(outcome: RevertOutcome): "ok" | "warn" | "danger" {
  if (outcome.steps.some((s) => s.result === "failed")) return "danger";
  if (outcome.steps.some((s) => s.result !== "reverted")) return "warn";
  return "ok";
}
