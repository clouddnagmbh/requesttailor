// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// What the secret rescue wizard shows and decides, without Svelte (wave 12, strand G).

import type { Environment, HistoryFinding, RescueResult } from "../commands/types";

/** `rt_app::rescue::check_variable`: what `{{…}}` can hold. */
export function validVariable(name: string): boolean {
  return name.length > 0 && name.length <= 100 && /^[A-Za-z_][A-Za-z0-9_.-]*$/.test(name);
}

/** The variable the scanner's hint suggests (``…add `githubToken`…``), else `secret`. */
export function suggestVariable(finding: HistoryFinding): string {
  for (const m of finding.finding.hint.matchAll(/`([^`]+)`/g)) {
    if (validVariable(m[1])) return m[1];
  }
  return "secret";
}

/** Private keys cannot become a variable (the rewrite refuses them). */
export function canRescue(finding: HistoryFinding): boolean {
  return !finding.finding.kind.includes("private key");
}

export function shortOid(oid: string): string {
  return oid.slice(0, 7);
}

/** The commands to run by hand for every rewritten branch with an upstream. */
export function forcePushCommands(result: RescueResult): string[] {
  return result.branches
    .filter((b) => b.pushed)
    .map((b) => `git push --force-with-lease ${b.remote ?? "origin"} ${b.name}`);
}

/** The commands that delete this rescue's backups (they still hold the value). */
export function deleteBackupCommands(result: RescueResult): string[] {
  return result.branches.map((b) => `git update-ref -d ${result.backup}${b.name}`);
}

/** `env` (or a new environment `name`) with `variable` declared as a secret, so the value in
 *  the secret store is used when sending; `null` when it already is. */
export function withSecretVariable(
  env: Environment | undefined,
  name: string,
  variable: string,
): Environment | null {
  const base: Environment = env ?? { name, variables: [], client_certificates: [], external_secrets: null };
  const existing = base.variables.find((v) => v.name === variable);
  if (existing?.secret && existing.enabled) return null;
  const declared = { name: variable, value: null, enabled: true, secret: true };
  return {
    ...base,
    variables: existing
      ? base.variables.map((v) => (v.name === variable ? declared : v))
      : [...base.variables, declared],
  };
}
