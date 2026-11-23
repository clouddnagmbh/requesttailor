// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { webAgents } from "./agents";
import type { Session } from "./web-backend";

describe("web agents.check", () => {
  it("asks the core with the path and environment", async () => {
    const calls: unknown[][] = [];
    const ws = {
      agentCheck: (...args: unknown[]) => {
        calls.push(args);
        return { effect: "write", verdict: { decision: "needs-approval" } };
      },
    };
    const agents = webAgents(() => ({ ws }) as unknown as Session);
    expect(await agents.check("a/b.yml", "PRD")).toEqual({
      effect: "write",
      verdict: { decision: "needs-approval" },
    });
    await agents.check("a/b.yml", null);
    expect(calls).toEqual([
      ["a/b.yml", "PRD"],
      ["a/b.yml", null],
    ]);
  });
});
