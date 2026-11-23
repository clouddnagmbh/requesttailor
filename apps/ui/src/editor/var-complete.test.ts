// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { VariableScope } from "../commands/types";
import {
  completionEdit,
  entriesFrom,
  layerBadge,
  matchName,
  openPlaceholder,
  overrideText,
  rankEntries,
  validName,
  valueText,
} from "./var-complete";

const scope: VariableScope = {
  variables: [
    { name: "baseUrl", value: "http://collection", source: { layer: "collection" }, secret: false, external: null, overridden: true },
    { name: "apiVersion", value: "v2", source: { layer: "collection" }, secret: false, external: null, overridden: false },
    { name: "dbPassword", value: null, source: { layer: "environment", name: "Local" }, secret: true, external: "one-password", overridden: false },
    { name: "baseUrl", value: "http://local", source: { layer: "environment", name: "Local" }, secret: false, external: null, overridden: false },
    { name: "token", value: null, source: { layer: "environment", name: "Local" }, secret: true, external: null, overridden: false },
    { name: "userId", value: "42", source: { layer: "folder", path: "api/users" }, secret: false, external: null, overridden: false },
    { name: "page", value: "1", source: { layer: "request" }, secret: false, external: null, overridden: false },
    { name: "session", value: "s1", source: { layer: "runtime" }, secret: false, external: null, overridden: false },
  ],
  undefined: ["missing", "baseUrl"],
};

describe("entriesFrom", () => {
  const entries = entriesFrom(scope);

  it("keeps every definition, marks overridden ones with the winner, then undefined names", () => {
    expect(entries.map((e) => e.name)).toEqual([
      "baseUrl",
      "apiVersion",
      "dbPassword",
      "baseUrl",
      "token",
      "userId",
      "page",
      "session",
      "missing",
    ]);
    expect(entries[0].overriddenBy).toEqual({ layer: "environment", name: "Local" });
    expect(entries[3].overriddenBy).toBeNull();
    expect(entries.at(-1)!.kind).toBe("undefined");
  });

  it("never carries a secret value", () => {
    const leaky: VariableScope = {
      variables: [{ name: "t", value: "leak", source: { layer: "environment", name: "E" }, secret: true, external: null, overridden: false }],
      undefined: [],
    };
    expect(JSON.stringify(entriesFrom(leaky))).not.toContain("leak");
  });
});

describe("matchName", () => {
  it("ranks prefix, word start, substring, then letters in order", () => {
    expect(matchName("base", "baseUrl")).toEqual({ rank: 0, score: 0, ranges: [0, 4] });
    expect(matchName("BASE", "baseUrl")!.rank).toBe(0);
    expect(matchName("url", "baseUrl")).toMatchObject({ rank: 1, ranges: [4, 7] });
    expect(matchName("rl", "baseUrl")!.rank).toBe(1);
    expect(matchName("url", "baseUrl")!.score).toBeLessThan(matchName("rl", "baseUrl")!.score);
    expect(matchName("bu", "baseUrl")).toMatchObject({ rank: 2, ranges: [0, 1, 4, 5] });
    expect(matchName("xyz", "baseUrl")).toBeNull();
    expect(matchName("", "anything")).toEqual({ rank: 0, score: 0, ranges: [] });
  });
});

describe("rankEntries", () => {
  const entries = entriesFrom(scope);

  it("lists all by name with the winner before overridden definitions; undefined last", () => {
    const names = rankEntries(entries, "").map((r) => `${r.entry.name}${r.entry.overriddenBy ? "*" : ""}`);
    expect(names).toEqual(["apiVersion", "baseUrl", "baseUrl*", "dbPassword", "page", "session", "token", "userId", "missing"]);
  });

  it("puts prefix matches first and filters fuzzily", () => {
    const withUser = entriesFrom({
      variables: [
        { name: "currentUser", value: "a", source: { layer: "collection" }, secret: false, external: null, overridden: false },
        { name: "userId", value: "1", source: { layer: "collection" }, secret: false, external: null, overridden: false },
        { name: "superUserName", value: "b", source: { layer: "collection" }, secret: false, external: null, overridden: false },
      ],
      undefined: [],
    });
    expect(rankEntries(withUser, "user").map((r) => r.entry.name)).toEqual(["userId", "superUserName", "currentUser"]);
    expect(rankEntries(entries, "pw").map((r) => r.entry.name)).toEqual(["dbPassword"]);
  });

  it("ranks 500 variables well within the 50 ms budget", () => {
    const many = entriesFrom({
      variables: Array.from({ length: 500 }, (_, i) => ({
        name: `var${i}_${i % 7 === 0 ? "token" : "value"}`,
        value: `value ${i}`,
        source: { layer: "environment" as const, name: "Local" },
        secret: false,
        external: null,
        overridden: false,
      })),
      undefined: [],
    });
    const start = performance.now();
    for (const q of ["", "v", "va", "var1", "tok"]) rankEntries(many, q);
    expect((performance.now() - start) / 5).toBeLessThan(10);
  });
});

describe("row texts", () => {
  const entries = entriesFrom(scope);
  const by = (name: string, layer?: string) => entries.find((e) => e.name === name && (!layer || e.source?.layer === layer))!;

  it("shows values truncated, secrets masked, external sources and undefined names", () => {
    expect(valueText(by("baseUrl", "environment"))).toBe("http://local");
    expect(valueText(by("token"))).toBe("••••");
    expect(valueText(by("dbPassword"))).toBe("from 1Password");
    expect(valueText(by("missing"))).toBe("not defined");
    const long = { ...by("page"), value: `${"x".repeat(60)}\n y` };
    expect(valueText(long)).toHaveLength(48);
    expect(valueText(long).endsWith("…")).toBe(true);
    expect(valueText({ ...by("page"), value: "" })).toBe("empty");
  });

  it("names the layer", () => {
    expect(layerBadge({ layer: "environment", name: "Local" })).toMatchObject({ label: "Env", detail: "Local" });
    expect(layerBadge({ layer: "folder", path: "api/users" })).toMatchObject({ label: "Folder", detail: "users", title: "Folder api/users" });
    expect(layerBadge({ layer: "collection" }).label).toBe("Collection");
    expect(layerBadge({ layer: "runtime" }).label).toBe("Runtime");
    expect(overrideText(by("baseUrl", "collection"))).toBe("Overridden by Environment Local");
    expect(overrideText(by("baseUrl", "environment"))).toBeNull();
  });
});

describe("openPlaceholder", () => {
  it("finds the {{ before the cursor", () => {
    expect(openPlaceholder("{{", 2)).toEqual({ start: 2, query: "" });
    expect(openPlaceholder("x {{ba", 6)).toEqual({ start: 4, query: "ba" });
    expect(openPlaceholder('{"a": "{{us', 11)).toEqual({ start: 9, query: "us" });
    expect(openPlaceholder("{{{", 3)).toEqual({ start: 3, query: "" });
  });

  it("is closed by } or whitespace, and needs two braces", () => {
    expect(openPlaceholder("{{a}} x", 7)).toBeNull();
    expect(openPlaceholder("{{a}", 4)).toBeNull();
    expect(openPlaceholder("{{a b", 5)).toBeNull();
    expect(openPlaceholder("{a", 2)).toBeNull();
    expect(openPlaceholder("plain", 5)).toBeNull();
  });
});

describe("completionEdit", () => {
  const apply = (line: string, col: number, name: string) => {
    const open = openPlaceholder(line, col)!;
    const e = completionEdit(line, open.start, col, name);
    const text = line.slice(0, e.from) + e.insert + line.slice(e.to);
    return `${text.slice(0, e.cursor)}|${text.slice(e.cursor)}`;
  };

  it("inserts name}}", () => {
    expect(apply("{{", 2, "baseUrl")).toBe("{{baseUrl}}|");
    expect(apply("{{ba/users", 4, "baseUrl")).toBe("{{baseUrl}}|/users");
    expect(apply('{"id": {{us"', 11, "userId")).toBe('{"id": {{userId}}|"');
  });

  it("completes an existing }} without doubling it", () => {
    expect(apply("{{}}", 2, "baseUrl")).toBe("{{baseUrl}}|");
    expect(apply("{{ba}}/x", 4, "baseUrl")).toBe("{{baseUrl}}|/x");
    expect(apply("{{baseU}}", 4, "token")).toBe("{{token}}|");
  });

  it("leaves a word after the cursor alone when the placeholder is not closed", () => {
    expect(apply("{{users", 2, "baseUrl")).toBe("{{baseUrl}}|users");
    expect(apply('{"a": {{}', 8, "x")).toBe('{"a": {{x}}|}');
  });
});

describe("validName", () => {
  it("accepts what {{name}} resolves", () => {
    expect(validName("baseUrl")).toBe(true);
    expect(validName("api.v2-key_1")).toBe(true);
    expect(validName("")).toBe(false);
    expect(validName(" a")).toBe(false);
    expect(validName("a}b")).toBe(false);
  });
});
