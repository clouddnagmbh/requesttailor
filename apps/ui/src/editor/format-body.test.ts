// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { Body } from "../commands/types";
import { canFormat, describeFormat, formatBody, onFormatRequest, requestFormat } from "./format-body";

const raw = (kind: "json" | "xml" | "text" | "sparql", data: string): Body => ({ type: "raw", kind, data });

describe("format body", () => {
  it("formats raw JSON and XML only", () => {
    expect(canFormat(raw("json", "{}"))).toBe(true);
    expect(canFormat(raw("xml", "<a/>"))).toBe(true);
    expect(canFormat(raw("text", "{}"))).toBe(false);
    expect(canFormat({ type: "none" })).toBe(false);
    expect(canFormat(undefined)).toBe(false);
    expect(formatBody(raw("text", "{}"))).toBeNull();
  });

  it("keeps placeholders byte-identical", () => {
    expect(formatBody(raw("json", '{"id": {{id}}, "name": "{{name}}"}'))).toEqual({
      kind: "formatted",
      body: raw("json", '{\n  "id": {{id}},\n  "name": "{{name}}"\n}'),
    });
    expect(formatBody(raw("xml", "<a><id>{{id}}</id></a>"))).toEqual({
      kind: "formatted",
      body: raw("xml", "<a>\n  <id>{{id}}</id>\n</a>"),
    });
  });

  it("reports formatted and malformed bodies without changing them", () => {
    const same = formatBody(raw("json", '{\n  "a": 1\n}'))!;
    expect(same).toEqual({ kind: "unchanged" });
    expect(describeFormat(same)).toBe("Already formatted");
    const bad = formatBody(raw("xml", "<a>"))!;
    expect(bad).toEqual({ kind: "malformed", language: "XML" });
    expect(describeFormat(bad)).toBe("Not well-formed XML: nothing changed");
  });

  it("hands format requests to the listening editor until it unsubscribes", () => {
    let calls = 0;
    const off = onFormatRequest(() => calls++);
    requestFormat();
    off();
    requestFormat();
    expect(calls).toBe(1);
  });
});
