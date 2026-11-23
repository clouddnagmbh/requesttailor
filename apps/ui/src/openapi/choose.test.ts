// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { SpecOverview } from "../commands/types";
import { safeName, filterGroups, groupOperations, groupState, initialChoices, requestName, serverErrors, serverRows, setMany, toOptions } from "./choose";

const op = (method: string, path: string, tags: string[], summary: string | null, id: string | null) => ({
  key: `${method} ${path}`,
  method,
  path,
  tags,
  summary,
  operation_id: id,
  deprecated: false,
});

const spec: SpecOverview = {
  title: "Petstore",
  version: "1.0.0",
  description: null,
  spec_version: "3.0.3",
  main_file: "petstore.yaml",
  files: 1,
  servers: [
    { url: "https://api.example.com", description: null, suggested_name: "api.example.com", variables: [] },
    { url: "https://staging.example.com", description: null, suggested_name: "staging.example.com", variables: [] },
    { url: "http://localhost:8080", description: null, suggested_name: "localhost:8080", variables: [] },
  ],
  tags: [
    { name: "pets", description: null, operations: 2 },
    { name: "store", description: null, operations: 1 },
  ],
  operations: [
    op("GET", "/pets", ["pets"], "List all pets", "listPets"),
    op("GET", "/pets/{petId}", ["pets"], "Info for a pet", "showPetById"),
    op("POST", "/store/orders", ["store"], null, "placeOrder"),
    op("GET", "/health", [], null, null),
  ],
  security: [],
  losses: [],
};

describe("choose", () => {
  it("groups by tag, path or flat", () => {
    expect(groupOperations(spec, "tag").map((g) => [g.label, g.operations.length])).toEqual([
      ["pets", 2],
      ["store", 1],
      ["Other", 1],
    ]);
    expect(groupOperations(spec, "path").map((g) => g.key)).toEqual(expect.arrayContaining(["pets", "store", "health"]));
    expect(groupOperations(spec, "flat")).toHaveLength(1);
  });

  it("names requests from summary or operationId", () => {
    expect(requestName(spec.operations[0]!, "summary")).toBe("List all pets");
    expect(requestName(spec.operations[0]!, "operation-id")).toBe("listPets");
    expect(requestName(spec.operations[2]!, "summary")).toBe("placeOrder");
    expect(requestName(spec.operations[3]!, "operation-id")).toBe("GET /health");
  });

  it("searches and selects whole groups", () => {
    const groups = filterGroups(groupOperations(spec, "tag"), "pet");
    expect(groups.map((g) => g.key)).toEqual(["pets"]);
    const none = setMany(new Set(), groups[0]!.operations, true);
    expect(groupState(groups[0]!, none)).toBe("all");
    expect(groupState(groups[0]!, new Set(["GET /pets"]))).toBe("some");
    expect(groupState(groups[0]!, new Set())).toBe("none");
  });

  it("suggests environment names and checks them", () => {
    const rows = serverRows(spec);
    expect(rows.map((r) => r.name)).toEqual(["api.example.com", "Staging", "Local"]);
    expect(serverErrors([{ ...rows[0]!, name: "" }])).toEqual({ 0: "Give the environment a name." });
    expect(serverErrors([rows[1]!, { ...rows[2]!, name: "staging" }])).toEqual({ 2: "Another environment has this name." });
  });

  it("keeps suggested names valid as environment names", () => {
    expect(safeName("shop.example.com/api")).toBe("shop.example.com");
    expect(safeName("api.example.com:8443")).toBe("api.example.com-8443");
    expect(safeName("/")).toBe("Server");
  });

  it("turns the choices into options; all operations is null", () => {
    const c = initialChoices(spec);
    expect(c.keepSpec).toBe(true);
    expect(toOptions(spec, c).operations).toBeNull();
    const two = { ...c, selected: new Set(["GET /pets", "POST /store/orders"]) };
    expect(toOptions(spec, two)).toMatchObject({
      operations: ["GET /pets", "POST /store/orders"],
      grouping: "tag",
      keep_spec: true,
      servers: [
        { index: 0, environment: "api.example.com" },
        { index: 1, environment: "Staging" },
        { index: 2, environment: "Local" },
      ],
    });
  });
});
