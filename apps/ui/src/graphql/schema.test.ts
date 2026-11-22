// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Completion and validation (cm6-graphql, graphql-language-service) are checked in the
// browser (apps/web/e2e/graphql), where one copy of graphql-js is bundled for both.
import { describe, expect, it } from "vitest";

import { MOCK_SDL, mockGraphQl } from "../commands/mock-graphql";
import { browse, parseSchema, variablesProblem } from "./schema";

describe("GraphQL schema", () => {
  const schema = parseSchema(MOCK_SDL);

  it("lists root types first, then by name, without built-ins", () => {
    const types = browse(schema);
    expect(types.map((t) => [t.name, t.kind, t.root])).toEqual([
      ["Query", "object", "query"],
      ["Mutation", "object", "mutation"],
      ["Book", "object", null],
      ["BookInput", "input", null],
      ["Genre", "enum", null],
    ]);
    const query = types[0];
    expect(query.description).toBe("The root of all queries.");
    expect(query.fields[0]).toEqual({
      name: "hello",
      args: "(name: String)",
      type: "String!",
      description: "Greets `name`.",
      deprecated: null,
    });
    expect(types[2].fields.find((f) => f.name === "year")?.deprecated).toBe("Use published.");
  });

  it("searches type names, descriptions and fields", () => {
    expect(browse(schema, "genre").map((t) => [t.name, t.fields.map((f) => f.name)])).toEqual([
      ["Query", ["books"]],
      ["Book", ["genre"]],
      ["BookInput", ["genre"]],
      ["Genre", ["FICTION", "SCIENCE"]],
    ]);
    expect(browse(schema, "library").map((t) => t.name)).toEqual(["Book"]);
    expect(browse(schema, "zzz")).toEqual([]);
  });

  it("checks variables", () => {
    expect(variablesProblem("")).toBeNull();
    expect(variablesProblem('{"a": 1}')).toBeNull();
    expect(variablesProblem("[1]")).toBe("Variables must be a JSON object.");
    expect(variablesProblem("{")).toMatch(/^Variables are not valid JSON/);
  });

  it("caches in the mock until refresh", async () => {
    const api = mockGraphQl();
    const input = { environment: "Local", vars: [] as [string, string][] };
    await api.schema("q.yml", input, false);
    await api.schema("q.yml", input, false);
    expect(api.fetched).toBe(1);
    await api.schema("q.yml", input, true);
    expect(api.fetched).toBe(2);
  });
});
