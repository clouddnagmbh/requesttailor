// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The mock mirror of `rt_app::views`: the same file bytes, the same shaping of a result. The
// core's own tests are `crates/rt-app/tests/views.rs`.

import { describe, expect, it } from "vitest";

import { MockBackend } from "../commands/mock";
import { companionPath, evaluate, renderFile } from "../commands/mock-views";
import type { ResponseView } from "../commands/types";

const v = (name: string, expression: string, display: ResponseView["display"], columns: string[] = []): ResponseView => ({
  name,
  language: "jsonpath",
  expression,
  display,
  ...(columns.length > 0 ? { columns } : {}),
});

const USERS = '{"items":[{"id":1,"name":"Ada","tags":["a"]},{"id":2,"name":"Bob","email":"b@x.test"},{"id":3,"name":"Cy"}],"total":3}';

describe("the views file", () => {
  it("is next to the request with the bytes the core writes", () => {
    expect(companionPath("users/get.yml")).toBe("users/get.views.json");
    expect(renderFile([v("Ids", "$.items[*].id", "json"), v("Users", "$.items", "table", ["id", "name"])])).toBe(
      '{\n  "version": 1,\n  "views": [\n    {\n      "name": "Ids",\n      "language": "jsonpath",\n      "expression": "$.items[*].id",\n      "display": "json"\n    },\n    {\n      "name": "Users",\n      "language": "jsonpath",\n      "expression": "$.items",\n      "display": "table",\n      "columns": [\n        "id",\n        "name"\n      ]\n    }\n  ]\n}\n',
    );
  });
});

describe("evaluating", () => {
  it("returns the matches of a JSON or text view", () => {
    expect(evaluate(USERS, v("n", "$.items[*].name", "json")).values).toEqual(["Ada", "Bob", "Cy"]);
    expect(evaluate(USERS, v("t", "$.total", "text")).values).toEqual([3]);
  });
  it("shapes a table with chosen columns or the first keys", () => {
    const r = evaluate(USERS, v("u", "$.items", "table", ["name", "id", "email"]));
    expect(r.columns).toEqual(["name", "id", "email"]);
    expect(r.rows?.[1]).toEqual(["Bob", 2, "b@x.test"]);
    expect(r.rows?.[0]?.[2]).toBeNull();
    expect(evaluate(USERS, v("u", "$.items[*]", "table")).columns).toEqual(["id", "name", "tags", "email"]);
  });
  it("explains what it cannot show", () => {
    expect(evaluate(USERS, v("t", "$.total", "table"))).toMatchObject({ display: "json", values: [3] });
    expect(evaluate(USERS, v("x", "$.nothing", "json")).notice).toContain("matched nothing");
    expect(evaluate("plain", v("x", "$", "json")).notice).toContain("not JSON");
    expect(() => evaluate(USERS, v("x", "$.[", "json"))).toThrow(/invalid JSONPath/);
    expect(() => evaluate(USERS, { ...v("j", ".a", "json"), language: "jq" })).toThrow(/jq is not available in this build/);
  });
});

describe("the mock backend", () => {
  it("stages the file, keeps names unique and removes", async () => {
    const b = new MockBackend();
    await b.openCollection();
    const path = "users/list-users.yml";
    expect(await b.views.list(path)).toEqual([]);
    await b.views.save(path, v("Ids", "$.method", "json"));
    await b.views.save(path, v("Ids", "$.url", "text", ["x"]));
    const list = await b.views.list(path);
    expect(list).toHaveLength(1);
    expect(list[0]).toMatchObject({ expression: "$.url", display: "text" });
    expect(list[0]?.columns ?? []).toEqual([]);
    expect(await b.companions.read(companionPath(path))).toBe(renderFile(list));
    expect(await b.views.remove(path, "Ids")).toEqual([]);
    expect(await b.views.languages()).toEqual(["jsonpath"]);
    await expect(b.views.save(path, v("", "$", "json"))).rejects.toThrow();
  });
});
