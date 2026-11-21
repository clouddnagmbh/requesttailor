// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { Param } from "../commands/types";
import { paramsFromUrl, parseQuery, pathParamNames, splitUrl, urlFromParams } from "./url";

const q = (name: string, value: string, enabled = true): Param => ({ name, value, kind: "query", enabled });
const p = (name: string, value: string, enabled = true): Param => ({ name, value, kind: "path", enabled });

describe("splitUrl", () => {
  it("splits base, query and fragment", () => {
    expect(splitUrl("http://h/a?x=1#top")).toEqual({ base: "http://h/a", query: "x=1", fragment: "#top" });
  });
  it("has no query without `?`", () => {
    expect(splitUrl("{{baseUrl}}/users")).toEqual({ base: "{{baseUrl}}/users", query: null, fragment: "" });
  });
  it("keeps an empty query after a trailing `?`", () => {
    expect(splitUrl("http://h/a?")).toEqual({ base: "http://h/a", query: "", fragment: "" });
  });
  it("ignores `?`, `#` inside {{ }}", () => {
    expect(splitUrl("{{host?x}}/a?b={{c#d}}")).toEqual({ base: "{{host?x}}/a", query: "b={{c#d}}", fragment: "" });
  });
  it("treats an unclosed {{ as plain text", () => {
    expect(splitUrl("http://h/{{a?b=1")).toEqual({ base: "http://h/{{a", query: "b=1", fragment: "" });
  });
  it("round-trips", () => {
    for (const url of ["", "?", "a?b", "h/x?a=1&&b=#f", "{{u}}?{{k}}={{v}}#", "x#y?z"]) {
      const s = splitUrl(url);
      expect(s.base + (s.query === null ? "" : "?" + s.query) + s.fragment).toBe(url);
    }
  });
});

describe("parseQuery", () => {
  it("parses pairs without decoding", () => {
    expect(parseQuery("a=1&b=x%20y&c=%26")).toEqual([
      { name: "a", value: "1", raw: "a=1" },
      { name: "b", value: "x%20y", raw: "b=x%20y" },
      { name: "c", value: "%26", raw: "c=%26" },
    ]);
  });
  it("handles empty values, flags and empty pieces", () => {
    expect(parseQuery("a=&flag&&=v")).toEqual([
      { name: "a", value: "", raw: "a=" },
      { name: "flag", value: "", raw: "flag" },
      { name: "", value: "v", raw: "=v" },
    ]);
  });
  it("keeps repeated names", () => {
    expect(parseQuery("id=1&id=2").map((x) => x.value)).toEqual(["1", "2"]);
  });
  it("splits value at the first `=` only", () => {
    expect(parseQuery("f=a=b")).toEqual([{ name: "f", value: "a=b", raw: "f=a=b" }]);
  });
  it("ignores `&` and `=` inside {{ }}", () => {
    expect(parseQuery("{{k=1}}={{x&y}}&b=2")).toEqual([
      { name: "{{k=1}}", value: "{{x&y}}", raw: "{{k=1}}={{x&y}}" },
      { name: "b", value: "2", raw: "b=2" },
    ]);
  });
});

describe("pathParamNames", () => {
  it("finds :segments after the origin", () => {
    expect(pathParamNames("{{baseUrl}}/users/:id/posts/:postId?x=:no")).toEqual(["id", "postId"]);
  });
  it("ignores ports and colons inside {{ }}", () => {
    expect(pathParamNames("http://localhost:8080/{{a:b}}/:x")).toEqual(["x"]);
  });
  it("finds OData-style keys", () => {
    expect(pathParamNames("http://h/Users(:id)/Orders(K1=:k1,K2=:k2)")).toEqual(["id", "k1", "k2"]);
  });
  it("deduplicates and ignores a bare colon", () => {
    expect(pathParamNames("h/:a/:/:a")).toEqual(["a"]);
  });
});

describe("paramsFromUrl (URL edited)", () => {
  it("mirrors the query and keeps nothing extra", () => {
    expect(paramsFromUrl("h/a?x=1&x=2", [])).toEqual([q("x", "1"), q("x", "2")]);
  });
  it("removes all query rows when the query is gone", () => {
    expect(paramsFromUrl("h/a", [q("x", "1")])).toEqual([]);
  });
  it("keeps disabled rows in place", () => {
    const old = [q("a", "1"), q("b", "2", false), q("c", "3")];
    expect(paramsFromUrl("h?a=1&x=9&c=3", old)).toEqual([q("a", "1"), q("b", "2", false), q("x", "9"), q("c", "3")]);
  });
  it("keeps path param values by name and adds new ones empty", () => {
    const old = [q("a", "1"), p("id", "42"), p("gone", "x")];
    expect(paramsFromUrl("{{u}}/:id/:new?a=1", old)).toEqual([q("a", "1"), p("id", "42"), p("new", "")]);
  });
  it("keeps disabled path params disabled", () => {
    expect(paramsFromUrl("h/:id", [p("id", "1", false)])).toEqual([p("id", "1", false)]);
  });
  it("drops blank draft rows", () => {
    expect(paramsFromUrl("h?a=1", [q("", ""), q("a", "1")])).toEqual([q("a", "1")]);
  });
});

describe("urlFromParams (rows edited)", () => {
  it("rewrites the query from enabled rows", () => {
    expect(urlFromParams("{{baseUrl}}/users?page=1", [q("page", "2"), q("size", "10")])).toBe(
      "{{baseUrl}}/users?page=2&size=10",
    );
  });
  it("adds a query to a URL without one", () => {
    expect(urlFromParams("h/a", [q("x", "1")])).toBe("h/a?x=1");
  });
  it("drops disabled rows and removes `?` when nothing is left", () => {
    expect(urlFromParams("h/a?x=1&y=2", [q("x", "1", false), q("y", "2")])).toBe("h/a?y=2");
    expect(urlFromParams("h/a?x=1#f", [q("x", "1", false)])).toBe("h/a#f");
  });
  it("re-enabling puts the pair back at its row position", () => {
    expect(urlFromParams("h?b=2", [q("a", "1"), q("b", "2")])).toBe("h?a=1&b=2");
  });
  it("keeps unchanged pairs verbatim (flags, encodings, templates)", () => {
    const url = "h?flag&e=a%2Fb&t={{x&y}}&n=1";
    const rows = paramsFromUrl(url, []);
    rows[3] = q("n", "2");
    expect(urlFromParams(url, rows)).toBe("h?flag&e=a%2Fb&t={{x&y}}&n=2");
  });
  it("matches unchanged pairs after a row is removed", () => {
    const url = "h?flag&a=1&b";
    const rows = paramsFromUrl(url, []).filter((r) => r.name !== "a");
    expect(urlFromParams(url, rows)).toBe("h?flag&b");
  });
  it("keeps repeated names and empty values", () => {
    expect(urlFromParams("h", [q("id", "1"), q("id", ""), q("id", "3")])).toBe("h?id=1&id=&id=3");
  });
  it("skips blank draft rows", () => {
    expect(urlFromParams("h?a=1", [q("a", "1"), q("", "")])).toBe("h?a=1");
  });
  it("escapes characters that would change the structure, outside {{ }}", () => {
    expect(urlFromParams("h", [q("a=b", "x&y#z"), q("t", "{{a&b}}")])).toBe("h?a%3Db=x%26y%23z&t={{a&b}}");
  });
  it("leaves path params alone and keeps `?` inside {{ }} in the base", () => {
    expect(urlFromParams("{{h?x}}/:id?a=1", [q("a", "2"), p("id", "7")])).toBe("{{h?x}}/:id?a=2");
  });
  it("round-trips URL → rows → URL unchanged", () => {
    for (const url of ["h", "h?", "h?a=1&&b=", "h?x=1&x=2#f", "{{u}}?{{k}}={{v}}", "h?%E2%9C%93=%2B"]) {
      const rows = paramsFromUrl(url, []);
      const back = urlFromParams(url, rows);
      // Only empty pieces may disappear; everything else is byte-identical.
      expect(paramsFromUrl(back, [])).toEqual(rows);
      if (!url.includes("&&") && !url.endsWith("?")) expect(back).toBe(url);
    }
  });
});
