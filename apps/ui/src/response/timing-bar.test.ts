// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: the one-line timing bar (status line of the response, history rows).

import { describe, expect, it } from "vitest";

import { durationBar, timingBar } from "./timing-bar";

describe("timingBar", () => {
  const timing = { dns: 5, connect: 10, tls: 20, ttfb: 340 };

  it("stacks DNS, connect, TLS, server wait and transfer over the total", () => {
    const bar = timingBar(timing, 410, "native", "https://api.example.com/x");
    expect(bar.segments.map((s) => s.name)).toEqual(["dns", "connect", "tls", "wait", "transfer"]);
    expect(bar.segments.map((s) => Math.round(s.ms))).toEqual([5, 10, 20, 305, 70]);
    expect(bar.total).toBe(410);
    const shares = bar.segments.reduce((sum, s) => sum + s.pct, 0);
    expect(shares).toBeCloseTo(100, 6);
  });

  it("says how much of the time the server took", () => {
    expect(timingBar(timing, 410, "native", "https://x").sentence).toBe("Server wait is 74 % of 410 ms");
    expect(timingBar({ dns: null, connect: null, tls: null, ttfb: 82 }, 100, "native", "http://x").sentence).toBe(
      "Server wait is 82 % of 100 ms",
    );
    expect(timingBar({ dns: 0, connect: 0, tls: 0, ttfb: 1000 }, 1200, "native", "https://x").sentence).toBe(
      "Server wait is 83 % of 1.20 s",
    );
  });

  it("leaves out what the transport cannot see and says why", () => {
    const bar = timingBar({ dns: null, connect: null, tls: null, ttfb: 90 }, 120, "browser", "https://x");
    expect(bar.segments.map((s) => s.name)).toEqual(["wait", "transfer"]);
    expect(bar.hidden.map((h) => h.name)).toEqual(["dns", "connect", "tls"]);
    expect(bar.hidden[0]!.reason).toBe("not visible in the browser");
    expect(bar.title).toContain("not visible in the browser");
  });

  it("copes with a zero total and a total shorter than the first byte", () => {
    const zero = timingBar({ dns: null, connect: null, tls: null, ttfb: 0 }, 0, "native", "http://x");
    expect(zero.sentence).toBe("Server wait is 0 % of 0.00 ms");
    expect(zero.segments.every((s) => s.pct === 0)).toBe(true);
    const odd = timingBar({ dns: null, connect: null, tls: null, ttfb: 50 }, 40, "native", "http://x");
    expect(odd.total).toBe(50);
  });
});

describe("durationBar (history rows)", () => {
  it("scales a duration against the slowest row", () => {
    expect(durationBar(50, 200)).toEqual({ pct: 25, label: "50 ms" });
    expect(durationBar(400, 200)?.pct).toBe(100);
    expect(durationBar(null, 200)).toBeNull();
    expect(durationBar(10, 0)).toEqual({ pct: 100, label: "10 ms" });
  });
});
