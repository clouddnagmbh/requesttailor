// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import { describeError, formatDuration, formatSize, statusClass, waterfall } from "./format";
import {
  arraySource,
  clampTop,
  hexLine,
  hexSource,
  LineCache,
  maxTop,
  thumb,
  topForThumb,
  visibleRange,
  type LineSource,
} from "./lines";

describe("format", () => {
  it("formats sizes", () => {
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1023)).toBe("1023 B");
    expect(formatSize(1536)).toBe("1.5 KB");
    expect(formatSize(100 * 1024 * 1024)).toBe("100 MB");
    expect(formatSize(5 * 1024 ** 3)).toBe("5 GB");
  });

  it("formats durations", () => {
    expect(formatDuration(0.25)).toBe("0.25 ms");
    expect(formatDuration(1.2)).toBe("1.2 ms");
    expect(formatDuration(14)).toBe("14 ms");
    expect(formatDuration(1234)).toBe("1.23 s");
  });

  it("classifies status codes", () => {
    expect([101, 200, 304, 404, 503].map(statusClass)).toEqual([
      "info",
      "ok",
      "redirect",
      "client-error",
      "server-error",
    ]);
  });

  it("describes errors", () => {
    expect(describeError({ kind: "transport", message: { kind: "timeout" } })).toBe("The request timed out.");
    expect(describeError({ kind: "prepare", message: "boom" })).toContain("boom");
  });

  it("builds the waterfall and hides phases the transport cannot see", () => {
    const w = waterfall({ dns: 1.2, connect: 2.5, tls: null, ttfb: 12.5 }, 14, "browser", "http://x");
    expect(w.phases.map((p) => [p.name, p.start, p.duration])).toEqual([
      ["dns", 0, 1.2],
      ["connect", 1.2, 2.5],
      ["ttfb", 3.7, 12.5 - 3.7],
      ["download", 12.5, 1.5],
    ]);
    expect(w.hidden).toEqual([{ name: "tls", label: "TLS", reason: "not visible in the browser" }]);
    const native = waterfall({ dns: null, connect: null, tls: null, ttfb: 5 }, 6, "native", "http://x");
    expect(native.hidden.map((h) => h.reason)).toEqual(["not measured", "not measured", "not used (plain HTTP)"]);
  });
});

describe("scroll model", () => {
  // 100 MB of 64-byte lines at 18 px: far beyond what a browser can lay out.
  const m = { count: 1_638_400, rowHeight: 18, viewport: 400 };

  it("reaches the last line", () => {
    const top = clampTop(m, maxTop(m) + 1e9);
    const { first, rows } = visibleRange(m, top);
    expect(first + rows).toBe(m.count);
    expect(rows).toBeLessThanOrEqual(Math.ceil(400 / 18) + 1);
  });

  it("maps the thumb onto the whole range", () => {
    const track = 400;
    const t = thumb(m, track, maxTop(m));
    expect(t.size).toBe(24);
    expect(t.offset).toBeCloseTo(track - 24);
    expect(topForThumb(m, track, track)).toBe(maxTop(m));
    expect(topForThumb(m, track, 0)).toBe(0);
    expect(topForThumb(m, track, (track - 24) / 2)).toBeCloseTo(maxTop(m) / 2);
  });

  it("handles content shorter than the viewport", () => {
    const small = { count: 3, rowHeight: 18, viewport: 400 };
    expect(maxTop(small)).toBe(0);
    expect(visibleRange(small, 0)).toEqual({ first: 0, rows: 3 });
    expect(thumb(small, 400, 0)).toEqual({ size: 400, offset: 0 });
    expect(visibleRange({ ...small, count: 0 }, 0)).toEqual({ first: 0, rows: 0 });
  });
});

describe("LineCache", () => {
  function counting(count: number): LineSource & { calls: number[] } {
    const calls: number[] = [];
    return {
      count,
      calls,
      get: async (start, n) => {
        calls.push(start);
        return { start, lines: Array.from({ length: n }, (_, k) => `line ${start + k}`), cut: n > 1 ? [1] : [] };
      },
    };
  }

  it("fetches chunks once and serves lines", async () => {
    const src = counting(1000);
    let loads = 0;
    const cache = new LineCache(src, () => loads++, 100, 4);
    expect(cache.get(5)).toBeUndefined();
    await Promise.all([cache.ensure(0, 150), cache.ensure(50, 120)]);
    expect(src.calls).toEqual([0, 100]);
    expect(loads).toBe(2);
    expect(cache.get(5)).toEqual({ text: "line 5", cut: false });
    expect(cache.get(101)).toEqual({ text: "line 101", cut: true });
  });

  it("evicts the least recently used chunks", async () => {
    const src = counting(10_000);
    const cache = new LineCache(src, () => {}, 100, 3);
    await cache.ensure(0, 0);
    await cache.ensure(100, 100);
    await cache.ensure(200, 200);
    await cache.ensure(0, 0); // touch chunk 0
    await cache.ensure(300, 300); // evicts chunk 1
    expect(cache.size).toBe(3);
    expect(cache.get(0)).toBeDefined();
    expect(cache.get(100)).toBeUndefined();
    for (let i = 0; i < 100; i++) await cache.ensure(i * 100, i * 100 + 50);
    expect(cache.size).toBe(3);
  });

  it("clamps the last chunk", async () => {
    const src = counting(250);
    const cache = new LineCache(src, () => {}, 100, 3);
    await cache.ensure(240, 10_000);
    expect(cache.get(249)?.text).toBe("line 249");
    expect(cache.get(250)).toBeUndefined();
  });

  it("serves arrays and hex dumps", async () => {
    expect((await arraySource(["a", "b", "c"]).get(1, 5)).lines).toEqual(["b", "c"]);
    const bytes = new Uint8Array(40).map((_, i) => i + 60);
    const hex = hexSource(40, async (o, n) => bytes.slice(o, o + n));
    expect(hex.count).toBe(3);
    const slice = await hex.get(2, 5);
    expect(slice.lines).toHaveLength(1);
    expect(slice.lines[0]).toBe(hexLine(32, bytes.slice(32)));
    expect(hexLine(0, new Uint8Array([0x41, 0x00]))).toMatch(/^00000000  41 00 .* A\.$/);
  });
});
