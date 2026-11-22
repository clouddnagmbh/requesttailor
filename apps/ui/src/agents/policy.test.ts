// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { approvalTitle, effective, policyOf, rowsOf, since } from "./policy";
import { MOCK_APPROVAL } from "../commands/mock-agents";

describe("agent policy rows", () => {
  it("shows every environment and keeps entries of missing ones", () => {
    const rows = rowsOf(
      {
        access: "read",
        environments: [
          { environment: "PRD", access: "write", production: true },
          { environment: "OLD", access: "none", production: false },
        ],
      },
      ["DEV", "PRD"],
    );
    expect(rows).toEqual([
      { environment: "DEV", access: "default", production: false, missing: false },
      { environment: "PRD", access: "write", production: true, missing: false },
      { environment: "OLD", access: "none", production: false, missing: true },
    ]);
  });

  it("writes only rows that differ from the default", () => {
    const rows = rowsOf({ access: "read", environments: [] }, ["DEV", "QA", "PRD"]);
    rows[1].access = "write";
    rows[2].production = true;
    expect(policyOf("none", rows)).toEqual({
      access: "none",
      environments: [
        { environment: "QA", access: "write", production: false },
        { environment: "PRD", access: "none", production: true },
      ],
    });
  });

  it("round-trips a policy", () => {
    const policy = {
      access: "write" as const,
      environments: [{ environment: "PRD", access: "write" as const, production: true }],
    };
    expect(policyOf(policy.access, rowsOf(policy, ["DEV", "PRD"]))).toEqual(policy);
  });

  it("explains what agents may do", () => {
    const row = { environment: "PRD", access: "default" as const, production: true, missing: false };
    expect(effective("write", row)).toBe("Writing requests wait for approval");
    expect(effective("read", row)).toBe("Reading requests only");
    expect(effective("none", row)).toBe("Hidden from agents");
    expect(effective("write", { ...row, production: false })).toBe("Reading and writing");
  });

  it("formats the banner line", () => {
    expect(approvalTitle(MOCK_APPROVAL)).toBe("POST https://api.example.com/users?api_key=**** in PRD");
    expect(approvalTitle({ ...MOCK_APPROVAL, environment: null })).toBe("POST https://api.example.com/users?api_key=****");
    expect(since(0, 5_000)).toBe("5s ago");
    expect(since(0, 125_000)).toBe("2 min ago");
    expect(since(0, 7_300_000)).toBe("2 h ago");
  });
});
