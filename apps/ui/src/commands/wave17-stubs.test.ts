// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { BackendError } from "./backend";
import { stubShare, stubSnapshots, stubStageSpec } from "./wave17-stubs";

async function kindOf(p: Promise<unknown>): Promise<{ kind: string; message: string }> {
  try {
    await p;
  } catch (e) {
    expect(e).toBeInstanceOf(BackendError);
    return { kind: (e as BackendError).kind, message: (e as Error).message };
  }
  throw new Error("expected a rejection");
}

describe("wave 17 placeholders", () => {
  it("reject with kind unsupported and name the strand", async () => {
    const snapshots = stubSnapshots();
    const share = stubShare();
    const calls: Array<[Promise<unknown>, string]> = [
      [snapshots.read("a.yml"), "strand B"],
      [snapshots.freeze("a.yml", 1), "strand B"],
      [snapshots.setIgnore("a.yml", []), "strand B"],
      [snapshots.check("a.yml", 1), "strand B"],
      [snapshots.list(), "strand B"],
      [share.link("a.yml"), "strand C"],
      [share.open("rt:1.x"), "strand C"],
      [share.copyAs("a.yml", "curl"), "strand C"],
      [share.qr("rt:1.x"), "strand C"],
      [stubStageSpec()("openapi: 3.0.3"), "strand A"],
    ];
    for (const [call, strand] of calls) {
      const e = await kindOf(call);
      expect(e.kind).toBe("unsupported");
      expect(e.message).toContain("is not implemented yet (wave 17, " + strand + ")");
    }
  });
});
