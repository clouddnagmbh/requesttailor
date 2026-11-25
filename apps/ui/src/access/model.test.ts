// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { AccessCheck, AccessVerdict } from "../commands/types";
import { headline, isBroken, otherEnvironments, skippedHint, tone, valueRows } from "./model";

const check = (verdict: AccessVerdict): AccessCheck => ({ path: "a.yml", verdict, findings: [], status_a: 200, status_b: 200 });

describe("access model", () => {
  it("tones and broken verdicts", () => {
    expect(tone({ kind: "ok" })).toBe("ok");
    expect(tone({ kind: "broken-object-access" })).toBe("danger");
    expect(tone({ kind: "broken-function-access" })).toBe("danger");
    expect(tone({ kind: "inconclusive" })).toBe("warn");
    expect(tone({ kind: "skipped", reason: "x" })).toBe("neutral");
    expect(isBroken({ kind: "broken-object-access" })).toBe(true);
    expect(isBroken({ kind: "ok" })).toBe(false);
    expect(isBroken({ kind: "skipped", reason: "x" })).toBe(false);
  });

  it("says the verdict in one line", () => {
    expect(headline(check({ kind: "broken-object-access" }), "alice", "bob")).toBe("bob can read what belongs to alice");
    expect(headline(check({ kind: "ok" }), "alice", "bob")).toContain("denied");
    expect(headline(check({ kind: "skipped", reason: "skipped (writes)" }), "a", "b")).toBe("Not checked: skipped (writes)");
  });

  it("hints at what the user can do about a skipped check", () => {
    expect(skippedHint("skipped (writes)")).toContain("Allow writes");
    expect(skippedHint("skipped (production)")).toContain("never");
    expect(skippedHint("no object reference found")).toContain("{{orderId}}");
    expect(skippedHint("refused by the send guard (r): x")).toBeNull();
  });

  it("lists values with a limit and the other environments", () => {
    const e = { profile: "A", status: 200, header_names: [], values: Array.from({ length: 15 }, (_, i) => [`$.f${i}`, "string"] as [string, string]) };
    const v = valueRows(e);
    expect(v.rows).toHaveLength(12);
    expect(v.more).toBe(3);
    expect(otherEnvironments(["a", "b", "c"], "b")).toEqual(["a", "c"]);
  });
});
