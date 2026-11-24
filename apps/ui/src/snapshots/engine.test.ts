// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { checkIgnore, compareResponse, freezeResponse, parseSnapshot, snapshotText, type MockResponseView } from "./engine";

const view = (body: string, extra: Partial<MockResponseView> = {}): MockResponseView => ({
  status: 200,
  headers: [
    ["Content-Type", "application/json"],
    ["Date", "now"],
    ["Set-Cookie", "sid=1"],
    ["ETag", '"v1"'],
  ],
  body,
  ...extra,
});

const BODY = '{"items":[{"id":1,"createdAt":"a"},{"id":2,"createdAt":"b"}],"total":2}';

describe("snapshot engine (mock model of rt_core::snapshot)", () => {
  it("keeps allowlisted headers only and masks secrets", () => {
    const s = freezeResponse(view('{"t":"s3cr3t","n":1}'), ["s3cr3t"]);
    expect(s.headers).toEqual({ "content-type": "application/json" });
    expect(s.body).toEqual({ t: "***", n: 1 });
    expect(Object.keys(JSON.parse(snapshotText(s)))).toEqual(["version", "status", "headers", "body", "ignore"]);
    expect(snapshotText(s).endsWith("}\n")).toBe(true);
    expect(parseSnapshot(snapshotText(s))).toEqual(s);
    expect(freezeResponse(view("{}"), [], { keepEtag: true }).headers.etag).toBe('"v1"');
  });

  it("lists every place that differs", () => {
    const s = freezeResponse(view(BODY), []);
    expect(compareResponse(s, view(BODY), [])).toEqual([]);
    const other = '{"items":[{"id":1,"createdAt":"a"},{"id":"2","createdAt":"b","x":true}],"total":3}';
    const diffs = compareResponse(s, view(other, { status: 404 }), []);
    const kinds = Object.fromEntries(diffs.map((d) => [d.path, d.kind]));
    expect(kinds).toEqual({ status: "changed", "body.items[1].id": "type", "body.items[1].x": "added", "body.total": "changed" });
    const shorter = compareResponse(s, view('{"items":[{"id":1,"createdAt":"a"}],"total":2}'), []);
    expect(shorter).toEqual([{ path: "body.items[1]", kind: "removed", expected: { id: 2, createdAt: "b" }, actual: null }]);
  });

  it("ignores paths with wildcards, plain paths and parents", () => {
    const base = freezeResponse(view(BODY), []);
    const other = view('{"items":[{"id":1,"createdAt":"x"},{"id":2,"createdAt":"y"}],"total":2}');
    expect(compareResponse(base, other, [])).toHaveLength(2);
    expect(compareResponse({ ...base, ignore: ["$.items[*].createdAt"] }, other, [])).toEqual([]);
    expect(compareResponse({ ...base, ignore: ["res.body.items[0].createdAt"] }, other, []).map((d) => d.path)).toEqual(["body.items[1].createdAt"]);
    expect(compareResponse({ ...base, ignore: ["$.items"] }, view('{"items":[],"total":2}'), [])).toEqual([]);
    expect(compareResponse({ ...base, ignore: ["res.status", "res.headers.content-type"] }, view(BODY, { status: 500, headers: [["content-type", "text/plain"]] }), [])).toEqual([]);
    expect(compareResponse({ ...base, ignore: ["$.items[*].createdAt.deeper"] }, other, [])).toHaveLength(2);
  });

  it("checks ignore paths", () => {
    expect(checkIgnore("items.x")).not.toBeNull();
    expect(checkIgnore("$.a[")).not.toBeNull();
    expect(checkIgnore("$.a[*].b['c d']")).toBeNull();
    expect(checkIgnore("res.headers.etag")).toBeNull();
  });
});
