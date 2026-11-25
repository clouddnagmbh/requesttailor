// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { RevertPlan, TreeNode } from "../commands/types";
import { entryTarget, outcomeTone, productionQuestion, requestChoices, resultLine, timeLabel } from "./model";

const tree: TreeNode = {
  kind: "folder",
  name: "demo",
  path: "",
  children: [
    { kind: "request", name: "Create", path: "create.yml", method: "POST" } as TreeNode,
    {
      kind: "folder",
      name: "orders",
      path: "orders",
      children: [{ kind: "request", name: "Remove", path: "orders/remove.yml", method: "DELETE" } as TreeNode],
    },
  ],
};

describe("journal model", () => {
  it("lists the requests of the tree with their folders", () => {
    expect(requestChoices(tree)).toEqual([
      { path: "create.yml", label: "Create" },
      { path: "orders/remove.yml", label: "orders / Remove" },
    ]);
    expect(requestChoices(null)).toEqual([]);
  });

  it("names the id first, then the location", () => {
    const base = { id: 1, session: "s", at: 0, request_path: "a", method: "POST", url: "u", status: 201 };
    expect(entryTarget({ ...base, resource_id: "42", location: "http://x/42" })).toBe("id 42");
    expect(entryTarget({ ...base, location: "http://x/42" })).toBe("http://x/42");
    expect(entryTarget(base)).toBeNull();
  });

  it("asks the production question with the number of planned steps", () => {
    const step = { entry_id: 1, method: "DELETE", url: "u", effect: "write", description: "" } as const;
    const plan: RevertPlan = {
      environment: "prod",
      production: true,
      steps: [
        { ...step, state: "planned" },
        { ...step, state: "not-reversible" },
        { ...step, state: "planned" },
      ],
    };
    expect(productionQuestion(plan)).toBe("Revert 2 changes in PRODUCTION?");
    expect(productionQuestion({ ...plan, steps: [plan.steps[0]!] })).toBe("Revert 1 change in PRODUCTION?");
  });

  it("words the results and picks a tone", () => {
    expect(resultLine("skipped-conflict", "left alone")).toBe("Skipped: changed by someone else: left alone");
    expect(resultLine("reverted", "HTTP 204")).toBe("Reverted (HTTP 204)");
    expect(outcomeTone({ steps: [{ entry_id: 1, result: "reverted", message: "" }] })).toBe("ok");
    expect(outcomeTone({ steps: [{ entry_id: 1, result: "skipped-conflict", message: "" }] })).toBe("warn");
    expect(outcomeTone({ steps: [{ entry_id: 1, result: "failed", message: "" }] })).toBe("danger");
  });

  it("shows the clock for today and the date for earlier days", () => {
    const now = new Date(2026, 10, 24, 12, 0, 0).getTime();
    expect(timeLabel(new Date(2026, 10, 24, 9, 5, 7).getTime(), now)).toBe("09:05:07");
    expect(timeLabel(new Date(2026, 10, 20, 9, 5, 7).getTime(), now)).toBe("2026-11-20 09:05:07");
  });
});
