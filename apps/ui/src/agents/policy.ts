// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The agent policy as rows of the policy editor (wave 8, strand C). One row per environment
// of the collection plus rows for policy entries whose environment no longer exists. A row
// with access "default" and no production flag has no entry in the file; it follows the
// collection default. Rules (rt_app::agents::verdict): none → agents cannot see or use the
// environment; read → only reading requests; write → writing too, and in a production
// environment every writing request waits for the user's approval.

import type { AgentAccess, AgentPolicy, AuditEntry, PendingApproval } from "../commands/types";

export type RowAccess = AgentAccess | "default";

export interface PolicyRow {
  environment: string;
  access: RowAccess;
  production: boolean;
  /** The environment is not in the collection (any more). */
  missing: boolean;
}

export const ACCESS_LABELS: Record<AgentAccess, string> = {
  none: "No access",
  read: "Read only",
  write: "Read and write",
};

export function rowsOf(policy: AgentPolicy, environments: readonly string[]): PolicyRow[] {
  const entries = new Map(policy.environments.map((e) => [e.environment, e]));
  const rows: PolicyRow[] = environments.map((environment) => {
    const e = entries.get(environment);
    return { environment, access: e?.access ?? "default", production: e?.production ?? false, missing: false };
  });
  for (const e of policy.environments) {
    if (!environments.includes(e.environment)) {
      rows.push({ environment: e.environment, access: e.access, production: e.production, missing: true });
    }
  }
  return rows;
}

export function policyOf(access: AgentAccess, rows: readonly PolicyRow[]): AgentPolicy {
  return {
    access,
    environments: rows
      .filter((r) => r.access !== "default" || r.production)
      .map((r) => ({
        environment: r.environment,
        access: r.access === "default" ? access : r.access,
        production: r.production,
      })),
  };
}

/** What an agent may do in a row's environment, for the editor's summary column. */
export function effective(access: AgentAccess, row: PolicyRow): string {
  const a = row.access === "default" ? access : row.access;
  if (a === "none") return "Hidden from agents";
  if (a === "read") return "Reading requests only";
  return row.production ? "Writing requests wait for approval" : "Reading and writing";
}

/** When an approval was asked for, as the banner shows it. */
export function since(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min ago` : `${Math.floor(m / 60)} h ago`;
}

/** One line of the banner: `POST https://… in PRD`. */
export function approvalTitle(p: PendingApproval): string {
  return `${p.method} ${p.url}${p.environment ? ` in ${p.environment}` : ""}`;
}

export const OUTCOME_LABELS: Record<AuditEntry["outcome"], string> = {
  read: "Read",
  sent: "Sent",
  refused: "Refused",
  denied: "Denied",
  "timed-out": "Timed out",
  failed: "Failed",
};
