// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { RunEvent, RunStep } from "../commands/types";
import { planOf, RunResults } from "../runner/model";
import { rateChip, secondsText, waitingText } from "./model";

describe("rate-limit chip", () => {
  it("shows what is left and when it resets", () => {
    const chip = rateChip({ limit: 60, remaining: 47, reset_ms: 12_000 }, 200);
    expect(chip.text).toBe("47 / 60 left, resets in 12 s");
    expect(chip.tone).toBe("ok");
  });

  it("rounds the reset up and warns when little is left, turns red at zero", () => {
    expect(rateChip({ limit: 100, remaining: 8, reset_ms: 1200 }, 200)).toMatchObject({
      text: "8 / 100 left, resets in 2 s",
      tone: "warn",
    });
    expect(rateChip({ limit: 100, remaining: 0, reset_ms: 30_000 }, 200)).toMatchObject({ tone: "bad" });
  });

  it("says retry-after on a 429", () => {
    expect(rateChip({ retry_after_ms: 12_000 }, 429)).toMatchObject({ text: "Rate limited, retry in 12 s", tone: "bad" });
    expect(rateChip({}, 429).text).toBe("Rate limited");
    expect(rateChip({ reset_ms: 3000 }, 429).text).toBe("Rate limited, resets in 3 s");
  });

  it("copes with partial headers and lists windows in the title", () => {
    expect(rateChip({ remaining: 3 }, 200).text).toBe("3 left");
    expect(rateChip({ reset_ms: 5000 }, 200).text).toBe("Resets in 5 s");
    expect(rateChip({ limit: 60, windows: [{ limit: 60, seconds: 60 }] }, 200)).toMatchObject({
      text: "Limit 60",
    });
    expect(rateChip({ limit: 60, windows: [{ limit: 60, seconds: 60 }] }, 200).title).toContain("60 per 60 s");
  });

  it("counts seconds up and never below zero", () => {
    expect(secondsText(0)).toBe("0 s");
    expect(secondsText(1)).toBe("1 s");
    expect(secondsText(-5)).toBe("0 s");
    expect(waitingText(10_500, 0)).toBe("Waiting 11 s (rate limit)");
    expect(waitingText(1000, 5000)).toBe("Waiting 0 s (rate limit)");
  });
});

describe("runner waiting", () => {
  const step = { iteration: 0, request_path: "a.yml", request_name: "a", passed: true, skipped: false, report: {} } as unknown as RunStep;

  it("the waiting event shows until the request or the step or the end", () => {
    const r = new RunResults();
    const wait: RunEvent = { kind: "waiting", request_path: "b.yml", until: 5000, reason: "rate limit: 0 left, resets in 5 s" };
    r.apply(wait);
    expect(r.waiting).toEqual({ request_path: "b.yml", until: 5000, reason: "rate limit: 0 left, resets in 5 s" });
    r.apply({ kind: "request", iteration: 0, request_path: "b.yml" });
    expect(r.waiting).toBeNull();
    r.apply(wait);
    r.apply({ kind: "step", step });
    expect(r.waiting).toBeNull();
    r.apply(wait);
    r.apply({ kind: "finished", summary: { requests: 1, passed: 1, failed: 0, skipped: 0, tests_passed: 0, tests_failed: 0, assertions_passed: 0, assertions_failed: 0, duration_ms: 1, stopped: false } });
    expect(r.waiting).toBeNull();
  });

  it("the plan asks for throttling only when switched on", () => {
    const base = { iterations: 1, delayMs: 0, bail: false, data: null };
    expect(planOf(["a.yml"], base).throttle).toBeUndefined();
    expect(planOf(["a.yml"], { ...base, respectLimits: false }).throttle).toBeUndefined();
    expect(planOf(["a.yml"], { ...base, respectLimits: true }).throttle).toEqual({ mode: "respect" });
  });
});
