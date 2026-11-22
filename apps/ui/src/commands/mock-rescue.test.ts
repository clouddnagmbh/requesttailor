// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock's secret rescue (wave 12, strand G): findings on the mock's commits, the shells'
// refusals, the value only in the secret store, and a clean scan afterwards.

import { describe, expect, it } from "vitest";

import { MockBackend } from "./mock";
import { MOCK_RESCUE_VALUE } from "./mock-rescue";
import type { ScanProgress } from "./types";

async function opened() {
  const b = new MockBackend();
  await b.openCollection();
  return b;
}

describe("mock rescue", () => {
  it("finds the token, refuses what the shells refuse and stores the value", async () => {
    const b = await opened();
    const progress: ScanProgress[] = [];
    const scan = await b.rescue.scanHistory((p) => progress.push(p));
    expect(scan.findings.map((f) => f.finding.kind)).toEqual(["GitHub token", "private key"]);
    expect(progress.at(-1)).toEqual({ scanned: scan.commits, total: scan.commits });
    expect(JSON.stringify(scan)).not.toContain(MOCK_RESCUE_VALUE);
    const token = scan.findings[0];
    const plan = { findings: [token.id], variable: "githubToken", environment: "Local" };

    await expect(b.rescue.rewrite({ ...plan, variable: "a b" })).rejects.toMatchObject({ kind: "git" });
    await expect(b.rescue.rewrite({ ...plan, findings: [scan.findings[1].id] })).rejects.toMatchObject({
      kind: "unsupported",
    });
    await b.createRequest("", "Draft");
    await expect(b.rescue.rewrite(plan)).rejects.toThrow(/Save or discard/);
    await b.save();

    const result = await b.rescue.rewrite(plan);
    expect(JSON.stringify(result)).not.toContain(MOCK_RESCUE_VALUE);
    expect(result.branches.find((x) => x.name === "main")).toMatchObject({ pushed: true, remote: "origin" });
    expect(result.backup).toMatch(/^refs\/requesttailor\/backup\/\d+\/$/);
    expect(await b.secrets.get("Local", "githubToken")).toBe(MOCK_RESCUE_VALUE);
    expect((await b.rescue.scanHistory()).findings.map((f) => f.finding.kind)).toEqual(["private key"]);
  });
});
