// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { checkIgnore } from "./engine";
import { hasIndex, ignorePath, show, stateLabel, withIgnore, without } from "./model";

describe("snapshot view model", () => {
  it("turns a difference path into an ignore path the engine accepts", () => {
    expect(ignorePath("body.items[0].id")).toBe("$.items[0].id");
    expect(ignorePath("body.items[0].id", true)).toBe("$.items[*].id");
    expect(ignorePath("body")).toBe("$");
    expect(ignorePath("body[0]")).toBe("$[0]");
    expect(ignorePath("status")).toBe("res.status");
    expect(ignorePath("headers.content-type")).toBe("res.headers.content-type");
    for (const p of ["body.a.b[1].c", "body", "status", "headers.etag", 'body["a b"].c']) {
      expect(checkIgnore(ignorePath(p))).toBeNull();
      expect(checkIgnore(ignorePath(p, true))).toBeNull();
    }
    expect(hasIndex("body.items[0].id")).toBe(true);
    expect(hasIndex("body.total")).toBe(false);
  });

  it("adds and removes ignored paths without duplicates", () => {
    expect(withIgnore(["$.a"], "$.b")).toEqual(["$.a", "$.b"]);
    expect(withIgnore(["$.a"], "$.a")).toEqual(["$.a"]);
    expect(without(["$.a", "$.b"], "$.a")).toEqual(["$.b"]);
  });

  it("words and values", () => {
    expect(stateLabel({ state: "matches", differences: [] })).toBe("Matches snapshot");
    expect(stateLabel({ state: "differs", differences: [{ path: "a", kind: "changed", expected: 1, actual: 2 }] })).toBe("1 difference from snapshot");
    expect(show(null)).toBe("nothing");
    expect(show("x")).toBe('"x"');
    expect(show("y".repeat(300)).length).toBeLessThanOrEqual(120);
  });
});
