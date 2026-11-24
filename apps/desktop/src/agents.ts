// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AgentsApi` of the desktop app (wave 8, strand C; src-tauri/src/agents.rs). The pending
// approvals live in the data folder, written by `rt mcp` processes: while the banner listens
// (`onPending`), the folder is read once a second and listeners hear about changes only.

import type { AgentsApi, PendingApproval } from "@requesttailor/ui";

import type { Call } from "./backend";

/** How often the approvals folder is read while someone listens. */
export const PENDING_POLL_MS = 1000;

export function tauriAgents(
  call: Call,
  timers: { every: (ms: number, f: () => void) => () => void } = {
    every: (ms, f) => {
      const id = setInterval(f, ms);
      return () => clearInterval(id);
    },
  },
): AgentsApi {
  const listeners = new Set<(pending: PendingApproval[]) => void>();
  let last = "[]";
  let stop: (() => void) | null = null;

  async function refresh(): Promise<PendingApproval[]> {
    const pending = await call<PendingApproval[]>("agents_pending");
    const key = JSON.stringify(pending);
    if (key !== last) {
      last = key;
      for (const l of listeners) l(pending);
    }
    return pending;
  }

  return {
    policy: () => call("agents_policy"),
    setPolicy: (policy) => call("agents_set_policy", { policy }),
    pending: () => refresh(),
    decide: async (id, decision) => {
      await call("agents_decide", { id, decision });
      await refresh().catch(() => undefined);
    },
    onPending(handler) {
      listeners.add(handler);
      if (last !== "[]") handler(JSON.parse(last) as PendingApproval[]);
      if (!stop) {
        stop = timers.every(PENDING_POLL_MS, () => void refresh().catch(() => undefined));
        void refresh().catch(() => undefined);
      }
      return () => {
        listeners.delete(handler);
        if (listeners.size === 0 && stop) {
          stop();
          stop = null;
        }
      };
    },
    check: (path, environment) => call("agents_check", { path, environment }),
    checkSpec: (spec, environment) => call("agents_check_spec", { spec, environment }),
    audit: (limit) => call("agents_audit", { limit }),
  };
}
