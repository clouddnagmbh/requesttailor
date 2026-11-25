// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pure helpers of the lint view (wave 18, strand F): the rules as the view names them, grouping
// of findings by rule and by folder, and the tone of the score. The rules themselves and the
// score formula are in the core (`rt_app::lint`); nothing is computed here.

import type { LintFinding, LintLevel, LintRule, LintSettings } from "../commands/types";

export interface RuleInfo {
  rule: LintRule;
  title: string;
  /** One line: what the rule looks for. */
  about: string;
  defaultLevel: LintLevel;
  weight: number;
}

export const RULES: RuleInfo[] = [
  { rule: "no-tests", title: "Requests without tests", about: "A request with no test, assertion or snapshot.", defaultLevel: "warn", weight: 40 },
  { rule: "hard-coded-host", title: "Hard-coded hosts", about: "A URL or token endpoint with a fixed host instead of a variable (loopback is only info).", defaultLevel: "warn", weight: 25 },
  { rule: "orphan-variable", title: "Unused variables", about: "A variable or secret that no request, script or assertion uses.", defaultLevel: "info", weight: 15 },
  { rule: "duplicate-name", title: "Duplicate names", about: "The same name or file name twice in one folder.", defaultLevel: "error", weight: 20 },
];

export const LEVELS: LintLevel[] = ["off", "info", "warn", "error"];

export function ruleInfo(rule: LintRule): RuleInfo {
  return RULES.find((r) => r.rule === rule)!;
}

/** The folder a finding belongs to (`""`: the collection). Environments are their own group. */
export function folderOf(path: string): string {
  if (path.startsWith("environments/")) return "environments";
  const trimmed = path.endsWith("/") ? path.slice(0, -1) : path;
  const at = trimmed.lastIndexOf("/");
  if (path.endsWith("/")) return trimmed;
  return at < 0 ? "" : trimmed.slice(0, at);
}

/** Whether a finding's path is a request file the editor can open. */
export function opensRequest(path: string): boolean {
  return path.endsWith(".yml") && !path.startsWith("environments/");
}

const RANK: Record<LintLevel, number> = { error: 0, warn: 1, info: 2, off: 3 };

function byWorst(a: LintFinding, b: LintFinding): number {
  return RANK[a.level] - RANK[b.level] || (a.path < b.path ? -1 : a.path > b.path ? 1 : 0);
}

export interface RuleGroup {
  rule: LintRule;
  findings: LintFinding[];
}

export function groupByRule(findings: LintFinding[]): RuleGroup[] {
  return RULES.map((r) => ({ rule: r.rule, findings: findings.filter((f) => f.rule === r.rule).sort(byWorst) })).filter((g) => g.findings.length > 0);
}

export interface FolderGroup {
  folder: string;
  findings: LintFinding[];
}

export function groupByFolder(findings: LintFinding[]): FolderGroup[] {
  const map = new Map<string, LintFinding[]>();
  for (const f of findings) {
    const key = folderOf(f.path);
    map.set(key, [...(map.get(key) ?? []), f]);
  }
  return [...map.keys()]
    .sort((a, b) => (a === "" ? -1 : b === "" ? 1 : a < b ? -1 : a > b ? 1 : 0))
    .map((folder) => ({ folder, findings: map.get(folder)!.sort(byWorst) }));
}

export function scoreTone(score: number): "ok" | "warn" | "danger" {
  return score >= 80 ? "ok" : score >= 50 ? "warn" : "danger";
}

export function levelOf(settings: LintSettings, rule: LintRule): LintLevel {
  return settings.rules[`lint/${rule}`] ?? ruleInfo(rule).defaultLevel;
}

/** The settings with `rule` at `level`; the default level removes the entry. */
export function withLevel(settings: LintSettings, rule: LintRule, level: LintLevel): LintSettings {
  const rules = { ...settings.rules };
  if (level === ruleInfo(rule).defaultLevel) delete rules[`lint/${rule}`];
  else rules[`lint/${rule}`] = level;
  return { ...settings, rules };
}

/** The open formula, as the help shows it. */
export const FORMULA =
  "For each rule, the satisfaction is ok / total over the items it checks. The score is the weighted mean over the rules that have something to check (tests 40, hosts 25, variables 15, names 20), rounded. Loopback URLs are reported but count as satisfied; ignored items are not checked.";
