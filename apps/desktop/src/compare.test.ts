// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompareApi` of the desktop shell (wave 12, strand C): every method calls its command with
// camelCase arguments, secret values never leave the webview, bisect steps arrive on a channel.

import type { BisectStep } from "@requesttailor/ui";
import { describe, expect, it } from "vitest";

import type { Command } from "./backend";
import { tauriCompare } from "./compare";

type Call = [Command, Record<string, unknown> | undefined];

function fake(answer: (cmd: Command) => unknown = () => null) {
  const calls: Call[] = [];
  const channels: ((step: BisectStep) => void)[] = [];
  const api = tauriCompare(
    async (cmd, args) => {
      calls.push([cmd, args as Record<string, unknown> | undefined]);
      return answer(cmd) as never;
    },
    (onStep) => {
      channels.push(onStep);
      return "step-channel";
    },
  );
  return { api, calls, channels };
}

// Secret values are not in the type, but a caller may pass more than it should.
const input = { environment: "Local", vars: [["v", "1"]] as [string, string][], secrets: [["token", "leak"]] } as never;

describe("tauriCompare", () => {
  it("maps every method to its command", async () => {
    const { api, calls } = fake();
    await api.run({ rev: "main", folder: "api" }, input);
    await api.compare({ from: "main", to: "feature" }, input);
    await api.bisect({ good: "v1", bad: "HEAD" }, input, () => {});
    await api.cancel();
    expect(calls).toEqual([
      ["compare_run", { request: { rev: "main", folder: "api" }, input: { environment: "Local", vars: [["v", "1"]] } }],
      ["compare_revisions", { request: { from: "main", to: "feature" }, input: { environment: "Local", vars: [["v", "1"]] } }],
      [
        "compare_bisect",
        { request: { good: "v1", bad: "HEAD" }, input: { environment: "Local", vars: [["v", "1"]] }, onStep: "step-channel" },
      ],
      ["compare_cancel", undefined],
    ]);
  });

  it("delivers the steps of a bisect from the channel", async () => {
    const { api, channels } = fake();
    const seen: BisectStep[] = [];
    await api.bisect({ good: "a", bad: "b" }, input, (s) => seen.push(s));
    const step = { commit: { oid: "1", summary: "", author: "", time: 0 }, verdict: "bad", failed: [], remaining: 2 } as BisectStep;
    channels[0]!(step);
    expect(seen).toEqual([step]);
  });
});
