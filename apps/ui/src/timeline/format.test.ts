// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { TestRecord } from "../commands/types";
import { headline, shortOid, testSummary } from "./format";

const record = (tests: { name: string; passed: boolean }[], passed: boolean, environment: string | null = "Local"): TestRecord => ({
  collection: "",
  request: "a.yml",
  environment,
  time: 1,
  source: "runner",
  passed,
  status: 200,
  tests,
});

describe("timeline labels", () => {
  it("counts versions and shortens ids", () => {
    expect(headline(1)).toBe("1 version");
    expect(headline(3)).toBe("3 versions");
    expect(shortOid("0123456789abcdef")).toBe("0123456");
  });

  it("summarises a recorded run", () => {
    expect(testSummary(record([{ name: "a", passed: true }, { name: "b", passed: false }], false))).toBe("1 of 2 tests passed · Local");
    expect(testSummary(record([], true, null))).toBe("no tests, no error");
    expect(testSummary(record([], false, null))).toBe("request failed");
  });
});
