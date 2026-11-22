// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RescueApi` of the desktop shell (wave 12, strand G): both methods call their command with
// camelCase arguments, the plan carries no more than ids, variable and environment, and the
// scan's progress arrives on a channel.

import type { ScanProgress } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import { COMMANDS, type Command } from "./backend";
import { PENDING, tauriRescue } from "./rescue";

type Call = [Command, Record<string, unknown> | undefined];

describe("tauriRescue", () => {
  it("maps both methods to their commands", async () => {
    const calls: Call[] = [];
    const channels: ((p: ScanProgress) => void)[] = [];
    const api = tauriRescue(
      async (cmd, args) => {
        calls.push([cmd, args as Record<string, unknown> | undefined]);
        return null as never;
      },
      (onProgress) => {
        channels.push(onProgress);
        return "progress-channel";
      },
    );
    const seen: ScanProgress[] = [];
    await api.scanHistory((p) => seen.push(p));
    // A caller may pass more than the plan holds; only the plan's fields go.
    await api.rewrite({ findings: ["a:b.yml:1"], variable: "apiToken", environment: "Local", value: "leak" } as never);
    expect(calls).toEqual([
      ["rescue_scan", { onProgress: "progress-channel" }],
      ["rescue_rewrite", { plan: { findings: ["a:b.yml:1"], variable: "apiToken", environment: "Local" } }],
    ]);
    channels[0]({ scanned: 3, total: 9 });
    expect(seen).toEqual([{ scanned: 3, total: 9 }]);
    await api.scanHistory();
    expect(channels).toHaveLength(2);
    expect(new Set(PENDING)).toEqual(new Set(calls.map(([c]) => c)));
    for (const c of PENDING) expect(COMMANDS).toContain(c);
  });
});
