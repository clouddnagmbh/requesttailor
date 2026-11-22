// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";
import type { PendingApproval } from "@requesttailor/ui";

import { tauriAgents } from "./agents";
import type { Call } from "./backend";

const approval = (id: string): PendingApproval => ({
  id,
  at: 1,
  agent: "claude-code",
  collection: "/c",
  collection_name: "Orders",
  request_path: "orders/create.yml",
  environment: "PRD",
  method: "POST",
  url: "https://prd/orders",
  reason: "writing request against the production environment PRD",
});

function fake() {
  let pending: PendingApproval[] = [];
  const calls: [string, unknown][] = [];
  const call = (async (cmd: string, args?: Record<string, unknown>) => {
    calls.push([cmd, args]);
    if (cmd === "agents_pending") return pending;
    if (cmd === "agents_decide") pending = pending.filter((p) => p.id !== args?.id);
    return null;
  }) as Call;
  let tick: (() => void) | null = null;
  const timers = {
    every: (_ms: number, f: () => void) => {
      tick = f;
      return () => (tick = null);
    },
  };
  return {
    call,
    calls,
    timers,
    set: (p: PendingApproval[]) => (pending = p),
    tick: () => tick?.(),
    running: () => tick !== null,
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

describe("desktop agents", () => {
  it("polls the approvals folder only while someone listens and reports changes", async () => {
    const f = fake();
    const agents = tauriAgents(f.call, f.timers);
    const seen: PendingApproval[][] = [];
    expect(f.running()).toBe(false);
    const stop = agents.onPending((p) => seen.push(p));
    expect(f.running()).toBe(true);
    await settle();
    // Nothing changed from the empty start: no call.
    expect(seen).toEqual([]);
    f.set([approval("a1")]);
    f.tick();
    await settle();
    f.tick();
    await settle();
    expect(seen).toEqual([[approval("a1")]]);
    await agents.decide("a1", "approved");
    expect(f.calls).toContainEqual(["agents_decide", { id: "a1", decision: "approved" }]);
    expect(seen.at(-1)).toEqual([]);
    stop();
    expect(f.running()).toBe(false);
  });

  it("forwards policy and audit", async () => {
    const f = fake();
    const agents = tauriAgents(f.call, f.timers);
    await agents.setPolicy({ access: "read", environments: [] });
    await agents.audit(50);
    expect(f.calls).toEqual([
      ["agents_set_policy", { policy: { access: "read", environments: [] } }],
      ["agents_audit", { limit: 50 }],
    ]);
  });
});
