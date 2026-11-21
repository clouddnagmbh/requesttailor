// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe as group, expect, it } from "vitest";

import { describe, findPlaceholders, infoFor, lookupFromPreview } from "./vars";

group("findPlaceholders", () => {
  it("matches like rt-core", () => {
    expect(findPlaceholders("{{a}}/{{}}/{{ b }}/{{{c}}/{{d}")).toEqual([
      { from: 0, to: 5, name: "a" },
      { from: 11, to: 18, name: " b " },
      { from: 19, to: 25, name: "{c" },
    ]);
  });
});

group("lookupFromPreview", () => {
  const lookup = lookupFromPreview({
    url: null,
    headers: [],
    variables: [
      { name: "baseUrl", value: "http://h", source: { layer: "environment", name: "Local" }, secret: false },
      { name: "token", value: "leak?", source: { layer: "environment", name: "Local" }, secret: true },
    ],
    unresolved: ["nope"],
    error: null,
  });

  it("classifies resolved, secret, unresolved and unknown names", () => {
    expect(infoFor(lookup, "baseUrl").state).toBe("resolved");
    expect(infoFor(lookup, "token")).toEqual({
      state: "secret",
      source: { layer: "environment", name: "Local" },
      value: null,
    });
    expect(infoFor(lookup, "nope").state).toBe("unresolved");
    expect(infoFor(lookup, "other").state).toBe("unknown");
  });

  it("describes provenance and never shows a secret value", () => {
    expect(describe("baseUrl", infoFor(lookup, "baseUrl"))).toEqual({
      title: "baseUrl · environment Local",
      detail: "http://h",
    });
    expect(describe("token", infoFor(lookup, "token")).detail).toBe("secret");
    expect(JSON.stringify(describe("token", infoFor(lookup, "token")))).not.toContain("leak");
    expect(
      describe("f", { state: "resolved", source: { layer: "folder", path: "users/admin" }, value: "" }),
    ).toEqual({ title: "f · folder users/admin", detail: "(empty)" });
  });
});
