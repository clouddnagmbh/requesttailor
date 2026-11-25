// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { LintFinding, LintSettings } from "../commands/types";
import {
  RULES,
  folderOf,
  groupByFolder,
  groupByRule,
  levelOf,
  opensRequest,
  scoreTone,
  withLevel,
} from "./model";

const finding = (rule: LintFinding["rule"], path: string, level: LintFinding["level"] = "warn"): LintFinding => ({
  rule,
  level,
  path,
  message: `${rule} ${path}`,
  location: null,
});

describe("lint model", () => {
  it("names the four rules with their default level", () => {
    expect(RULES.map((r) => r.rule)).toEqual(["no-tests", "hard-coded-host", "orphan-variable", "duplicate-name"]);
    expect(RULES.map((r) => r.defaultLevel)).toEqual(["warn", "warn", "info", "error"]);
    expect(RULES.map((r) => r.weight)).toEqual([40, 25, 15, 20]);
  });

  it("folderOf puts requests, folders, the collection and environments in a group", () => {
    expect(folderOf("users/get.yml")).toBe("users");
    expect(folderOf("a/b/get.yml")).toBe("a/b");
    expect(folderOf("get.yml")).toBe("");
    expect(folderOf("users/")).toBe("users");
    expect(folderOf("")).toBe("");
    expect(folderOf("environments/Dev.yml")).toBe("environments");
  });

  it("opens a request for a request path only", () => {
    expect(opensRequest("users/get.yml")).toBe(true);
    expect(opensRequest("users/")).toBe(false);
    expect(opensRequest("")).toBe(false);
    expect(opensRequest("environments/Dev.yml")).toBe(false);
  });

  it("groups findings by rule in the rule order, worst first inside", () => {
    const groups = groupByRule([
      finding("duplicate-name", "a.yml", "error"),
      finding("no-tests", "b.yml", "info"),
      finding("no-tests", "a.yml", "warn"),
    ]);
    expect(groups.map((g) => g.rule)).toEqual(["no-tests", "duplicate-name"]);
    expect(groups[0].findings.map((f) => f.path)).toEqual(["a.yml", "b.yml"]);
  });

  it("groups findings by folder, the collection first", () => {
    const groups = groupByFolder([finding("no-tests", "users/a.yml"), finding("no-tests", "z.yml"), finding("no-tests", "a/b.yml")]);
    expect(groups.map((g) => g.folder)).toEqual(["", "a", "users"]);
  });

  it("scoreTone is ok from 80, warn from 50", () => {
    expect(scoreTone(100)).toBe("ok");
    expect(scoreTone(80)).toBe("ok");
    expect(scoreTone(79)).toBe("warn");
    expect(scoreTone(50)).toBe("warn");
    expect(scoreTone(49)).toBe("danger");
  });

  it("reads and writes a level; the default level removes the entry", () => {
    const settings: LintSettings = { rules: {}, ignore: [], min_score: null };
    expect(levelOf(settings, "no-tests")).toBe("warn");
    const off = withLevel(settings, "no-tests", "off");
    expect(off.rules).toEqual({ "lint/no-tests": "off" });
    expect(levelOf(off, "no-tests")).toBe("off");
    expect(withLevel(off, "no-tests", "warn").rules).toEqual({});
    // The input is not changed.
    expect(settings.rules).toEqual({});
  });
});
