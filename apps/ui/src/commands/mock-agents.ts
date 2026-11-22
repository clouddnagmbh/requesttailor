// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AgentsApi` in the mock backend (wave 8, strand C). The policy lives in memory. Pending
// approvals are scripted: `?backend=mock&agents=1` starts with one writing request against
// PRD waiting, and UI tests add more through `request()` (`window.__rt.backend.agents`).
// Every decision lands in the audit log, like `rt mcp` would write it.

import type { AgentsApi } from "./backend";
import { BackendError } from "./backend";
import type { AgentPolicy, AuditEntry, PendingApproval } from "./types";

export interface MockAgents extends AgentsApi {
  /** Lets an approval wait, as an `rt mcp` process would. */
  request(approval: Partial<PendingApproval>): PendingApproval;
  /** The decisions made so far, oldest first. */
  readonly decisions: { id: string; decision: string }[];
}

/** `?agents=1` in the page URL (none outside a browser). */
function agentsParam(): boolean {
  try {
    return new URLSearchParams(globalThis.location?.search ?? "").get("agents") === "1";
  } catch {
    return false;
  }
}

export const MOCK_APPROVAL: PendingApproval = {
  id: "0123456789abcdef0123456789abcdef",
  at: 1_790_000_000_000,
  agent: "claude-code",
  collection: "mock://demo",
  collection_name: "Demo API",
  request_path: "users/create-user.yml",
  environment: "PRD",
  method: "POST",
  url: "https://api.example.com/users?api_key=****",
  reason: "writing request against the production environment PRD",
};

export function mockAgents(options: { scripted?: boolean } = {}): MockAgents {
  let policy: AgentPolicy = { access: "read", environments: [] };
  let pending: PendingApproval[] = (options.scripted ?? agentsParam()) ? [{ ...MOCK_APPROVAL }] : [];
  const audit: AuditEntry[] = [];
  const decisions: { id: string; decision: string }[] = [];
  const listeners = new Set<(p: PendingApproval[]) => void>();
  let next = 1;

  const notify = () => {
    const copy = pending.map((p) => ({ ...p }));
    for (const l of listeners) l(copy);
  };

  return {
    decisions,
    request(partial) {
      const approval = { ...MOCK_APPROVAL, id: `${next++}`.padStart(32, "0"), at: Date.now(), ...partial };
      pending = [...pending, approval];
      notify();
      return approval;
    },
    async policy() {
      return structuredClone(policy);
    },
    async setPolicy(p) {
      const names = p.environments.map((e) => e.environment);
      if (names.some((n) => n.trim() === "") || new Set(names).size !== names.length) {
        throw new BackendError("format", "environment names in the agent policy must be unique and not empty");
      }
      policy = structuredClone(p);
    },
    async pending() {
      return pending.map((p) => ({ ...p }));
    },
    async decide(id, decision) {
      const found = pending.find((p) => p.id === id);
      if (!found) throw new BackendError("shell", `no pending approval ${id}`);
      pending = pending.filter((p) => p.id !== id);
      decisions.push({ id, decision });
      audit.push({
        at: Date.now(),
        agent: found.agent,
        tool: "send_request",
        collection: found.collection,
        request_path: found.request_path,
        environment: found.environment ?? undefined,
        method: found.method,
        url: found.url,
        status: decision === "approved" ? 201 : undefined,
        outcome: decision === "approved" ? "sent" : "denied",
        approved_by: decision === "approved" ? "app" : undefined,
        detail: decision === "approved" ? "" : "denied in the app",
      });
      notify();
    },
    onPending(handler) {
      listeners.add(handler);
      handler(pending.map((p) => ({ ...p })));
      return () => listeners.delete(handler);
    },
    async audit(limit) {
      return [...audit].reverse().slice(0, limit);
    },
  };
}
