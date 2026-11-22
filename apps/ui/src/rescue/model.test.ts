// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { HistoryFinding, RescueResult } from "../commands/types";
import {
  canRescue,
  deleteBackupCommands,
  forcePushCommands,
  suggestVariable,
  validVariable,
  withSecretVariable,
} from "./model";

const finding = (kind: string, hint: string): HistoryFinding => ({
  id: `${"a".repeat(40)}:get.yml:3`,
  finding: { path: "get.yml", line: 3, kind, snippet: "token: ***", hint },
  first: { oid: "a".repeat(40), summary: "Add", author: "Ada", time: 0 },
  commits: ["a".repeat(40)],
  branches: ["main"],
  in_head: true,
});

describe("rescue model", () => {
  it("suggests the scanner's variable and checks names", () => {
    expect(suggestVariable(finding("GitHub token", "in Environments add `githubToken`, mark it"))).toBe("githubToken");
    expect(suggestVariable(finding("password", "Add `# rt-scan:allow` to the line."))).toBe("secret");
    expect(validVariable("api.token-2")).toBe(true);
    for (const bad of ["", "1x", "a b", "{{x}}"]) expect(validVariable(bad)).toBe(false);
    expect(canRescue(finding("private key", ""))).toBe(false);
    expect(canRescue(finding("bearer token", ""))).toBe(true);
  });

  it("lists the force pushes and backup deletions", () => {
    const result: RescueResult = {
      rewritten: 3,
      branches: [
        { name: "feature", old: "1", new: "2", pushed: false, remote: null },
        { name: "main", old: "3", new: "4", pushed: true, remote: "upstream" },
      ],
      backup: "refs/requesttailor/backup/1795000000/",
      tags: [],
    };
    expect(forcePushCommands(result)).toEqual(["git push --force-with-lease upstream main"]);
    expect(deleteBackupCommands(result)).toEqual([
      "git update-ref -d refs/requesttailor/backup/1795000000/feature",
      "git update-ref -d refs/requesttailor/backup/1795000000/main",
    ]);
  });

  it("declares the variable as a secret once", () => {
    const env = {
      name: "Local",
      variables: [{ name: "githubToken", value: "x", enabled: true, secret: false }],
      client_certificates: [],
      external_secrets: null,
    };
    const updated = withSecretVariable(env, "Local", "githubToken");
    expect(updated?.variables).toEqual([{ name: "githubToken", value: null, enabled: true, secret: true }]);
    expect(withSecretVariable(updated ?? undefined, "Local", "githubToken")).toBeNull();
    expect(withSecretVariable(undefined, "CI", "t")).toEqual({
      name: "CI",
      variables: [{ name: "t", value: null, enabled: true, secret: true }],
      client_certificates: [],
      external_secrets: null,
    });
  });
});
