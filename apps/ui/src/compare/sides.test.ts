// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { formatBytes, headerRows, statusClass, timeBars } from "./sides";

describe("answers side by side", () => {
  it("lines up the headers of both sides", () => {
    const rows = headerRows(
      [
        ["Content-Type", "application/json"],
        ["X-Version", "1"],
        ["Date", "Mon"],
        ["Set-Cookie", "a=***"],
        ["set-cookie", "b=***"],
        ["X-Old", "yes"],
      ],
      [
        ["content-type", "application/json"],
        ["X-Version", "2"],
        ["Date", "Tue"],
        ["X-New", "1"],
      ],
    );
    const by = Object.fromEntries(rows.map((r) => [r.key, r]));
    expect(rows.map((r) => r.key)).toEqual(["content-type", "date", "set-cookie", "x-new", "x-old", "x-version"]);
    expect(by["content-type"]!.kind).toBe("same");
    expect(by["x-version"]).toMatchObject({ kind: "changed", old: "1", new: "2" });
    expect(by["date"]).toMatchObject({ kind: "same", volatile: true });
    expect(by["set-cookie"]).toMatchObject({ old: "a=***, b=***", new: null, kind: "removed" });
    expect(by["x-new"]!.kind).toBe("added");
    // One side without an answer: nothing counts as changed.
    expect(headerRows(null, [["X", "1"]])[0]!.kind).toBe("same");
  });

  it("draws time bars against the slower side", () => {
    const t = timeBars(100, 250);
    expect(t.old).toEqual({ ms: 100, percent: 40 });
    expect(t.new).toEqual({ ms: 250, percent: 100 });
    expect(t.slower).toBe(true);
    expect(t.delta).toBe("+150 ms (2.5× slower)");
    const f = timeBars(300, 290);
    expect(f.slower || f.faster).toBe(false);
    expect(f.delta).toBe("−10 ms");
    expect(timeBars(0, 0).delta).toBe("Same time");
    expect(timeBars(null, 30)).toMatchObject({ old: null, new: { ms: 30, percent: 100 }, delta: null });
    expect(timeBars(1, 400).old!.percent).toBe(2);
  });

  it("formats sizes and status", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(2048)).toBe("2.0 KB");
    expect(formatBytes(50 * 1024)).toBe("50 KB");
    expect(formatBytes(3 * 1024 * 1024)).toBe("3.0 MB");
    expect([statusClass(204), statusClass(304), statusClass(404), statusClass(503)]).toEqual(["ok", "warn", "bad", "bad"]);
  });
});
