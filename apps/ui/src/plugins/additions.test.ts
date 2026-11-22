// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { emptyRequest, MockBackend } from "../commands/mock";
import type { TreeNode } from "../commands/types";
import { applyAdditions, findFolder, freeName, hasAdditions, isEmptyPlan, planAdditions } from "./additions";

const tree: TreeNode = {
  kind: "folder",
  name: "API",
  path: "",
  children: [
    {
      kind: "folder",
      name: "Sales Orders",
      path: "sales-orders",
      children: [
        { kind: "request", name: "Get order", path: "sales-orders/get-order.yml", method: "GET" },
        { kind: "request", name: "Get order 2", path: "sales-orders/get-order-2.yml", method: "GET" },
      ],
    },
    { kind: "request", name: "Ping", path: "ping.yml", method: "GET" },
  ],
};

const env = (name: string, vars: [string, string | null, boolean][]) => ({
  name,
  variables: vars.map(([n, value, secret]) => ({ name: n, value, enabled: true, secret })),
  client_certificates: [],
  external_secrets: null,
});

describe("plugin additions", () => {
  it("finds folders by directory or display name", () => {
    expect(findFolder(tree, "")?.path).toBe("");
    expect(findFolder(tree, "sales-orders")?.name).toBe("Sales Orders");
    expect(findFolder(tree, "Sales Orders/")?.path).toBe("sales-orders");
    expect(findFolder(tree, "missing")).toBeNull();
  });

  it("suffixes taken names", () => {
    const taken = new Set(["get order", "get order 2"]);
    expect(freeName("Get order", taken)).toBe("Get order 3");
    expect(freeName("New", taken)).toBe("New");
    expect(freeName("  ", new Set())).toBe("Request");
  });

  it("plans requests with suffixes, per folder and within the plan", () => {
    const spec = emptyRequest("x");
    const plan = planAdditions(
      {
        create_requests: [
          { folder: "Sales Orders", name: "Get order", spec },
          { folder: "sales-orders", name: "Get order", spec },
          { folder: "", name: "Ping", spec },
          { folder: "New/Deep", name: "Ping", spec },
        ],
      },
      tree,
      [],
    );
    expect(plan.requests.map((r) => [r.folder, r.name, r.renamed])).toEqual([
      ["Sales Orders", "Get order 3", true],
      ["sales-orders", "Get order 4", true],
      ["", "Ping 2", true],
      ["New/Deep", "Ping", false],
    ]);
    expect(plan.requests[0]!.spec.name).toBe("Get order 3");
  });

  it("adds only missing variables and never secret values", () => {
    const plan = planAdditions(
      {
        add_environments: [
          {
            name: "DEV",
            variables: [
              { name: "baseUrl", value: "https://other", secret: false },
              { name: "client", value: "100", secret: false },
            ],
          },
          { name: "QAS", variables: [{ name: "password", value: "leaked?", secret: true }, { name: "password", value: "", secret: true }] },
          { name: "PRD", variables: [{ name: "baseUrl", value: "https://prd", secret: false }] },
        ],
      },
      tree,
      [env("DEV", [["baseUrl", "https://dev", false]]), env("PRD", [["baseUrl", "https://prd", false]])],
    );
    expect(plan.environments).toEqual([
      { name: "DEV", created: false, variables: [{ name: "client", value: "100", enabled: true, secret: false }] },
      { name: "QAS", created: true, variables: [{ name: "password", value: null, enabled: true, secret: true }] },
    ]);
    expect(isEmptyPlan(planAdditions({ add_environments: [{ name: "PRD", variables: [] }] }, tree, [env("PRD", [])]))).toBe(true);
    expect(hasAdditions({ create_requests: [], add_environments: [] })).toBe(false);
  });

  it("applies a plan as unsaved changes", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const before = await b.environments();
    const plan = planAdditions(
      {
        create_requests: [
          { folder: "users", name: "List users", spec: { ...emptyRequest("List users"), url: "{{baseUrl}}/v2/users" } },
          { folder: "Sales Orders", name: "List orders", spec: emptyRequest("List orders") },
        ],
        add_environments: [
          { name: before[0]!.name, variables: [{ name: "sapClient", value: "100", secret: false }] },
          { name: "S4 DEV", variables: [{ name: "password", value: "x", secret: true }] },
        ],
      },
      await b.tree(),
      before,
    );
    const paths = await applyAdditions(b, plan);
    expect(paths).toEqual(["users/list-users-2.yml", "Sales Orders/list-orders.yml"]);
    expect((await b.getRequest(paths[0]!)).name).toBe("List users 2");
    expect((await b.getRequest(paths[0]!)).url).toBe("{{baseUrl}}/v2/users");
    const envs = await b.environments();
    expect(envs[0]!.variables.map((v) => v.name)).toEqual([...before[0]!.variables.map((v) => v.name), "sapClient"]);
    expect(envs.find((e) => e.name === "S4 DEV")?.variables).toEqual([{ name: "password", value: null, enabled: true, secret: true }]);
    expect(await b.dirty()).toEqual(expect.arrayContaining([...paths, `environments/${before[0]!.name}.yml`, "environments/S4 DEV.yml"]));
  });
});
