// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it, vi } from "vitest";

import type { FoundToken, JwtInfo, Observation } from "../commands/types";
import { startCountdown, type TickEnv } from "./countdown";
import { chipLabel, chipTone, countdown, locationText, relative, sendWarning, sentence, sortObservations, span, ticksFast, timeText } from "./model";

const obs = (id: string, severity: "info" | "warn"): Observation => ({ id, severity, location: "response headers", why: "Why.", fix: "Fix." });

const info = (warnings: JwtInfo["warnings"]): JwtInfo => ({
  header: { alg: "RS256" },
  algorithm: "RS256",
  claims: [],
  warnings,
  expires_at: null,
  signature_verified: false,
});

describe("the checks chip", () => {
  it("counts and is calm unless a warning is in it", () => {
    expect(chipLabel(1)).toBe("1 check");
    expect(chipLabel(4)).toBe("4 checks");
    expect(chipTone([obs("a", "info")])).toBe("info");
    expect(chipTone([obs("a", "info"), obs("b", "warn")])).toBe("warn");
  });
  it("lists warnings first and keeps the rule order otherwise", () => {
    const sorted = sortObservations([obs("a", "info"), obs("b", "warn"), obs("c", "info"), obs("d", "warn")]);
    expect(sorted.map((o) => o.id)).toEqual(["b", "d", "a", "c"]);
  });
});

describe("times", () => {
  it("rounds spans down", () => {
    expect([span(59), span(-125), span(7300), span(3 * 86_400 + 5)]).toEqual(["59 s", "2 min", "2 h", "3 d"]);
  });
  it("says when, in both directions", () => {
    expect(relative(720)).toBe("in 12 min");
    expect(relative(-720)).toBe("12 min ago");
    expect(relative(0)).toBe("now");
  });
  it("gives the date and the relative time", () => {
    expect(timeText(1_800_000_000, 1_800_000_000 + 3600)).toEqual({ date: "2027-01-15T08:00:00Z", relative: "1 h ago" });
  });
  it("counts down to the second under an hour", () => {
    expect(countdown(1000 + 75, 1000)).toBe("expires in 1 min 15 s");
    expect(countdown(1000 + 20, 1000)).toBe("expires in 20 s");
    expect(countdown(1000 + 7200, 1000)).toBe("expires in 2 h");
    expect(countdown(1000, 1000 + 125)).toBe("expired 2 min ago");
    expect(ticksFast(1000 + 90, 1000)).toBe(true);
    expect(ticksFast(1000 + 90_000, 1000)).toBe(false);
  });
});

describe("the warning before sending", () => {
  const where = { kind: "authorization" } as const;
  it("is about expired, not-yet-valid and nearly expired tokens", () => {
    for (const kind of ["expired", "not-yet-valid", "expires-soon"] as const) {
      const found: FoundToken[] = [{ location: where, token: info([{ kind, text: "token expired 12 min ago (exp)" }]) }];
      expect(sendWarning(found)?.warning.kind).toBe(kind);
    }
  });
  it("stays quiet for a good token, alg none and no token", () => {
    expect(sendWarning([])).toBeNull();
    expect(sendWarning([{ location: where, token: info([]) }])).toBeNull();
    expect(sendWarning([{ location: where, token: info([{ kind: "algorithm-none", text: "x" }]) }])).toBeNull();
  });
  it("makes a sentence without the claim name", () => {
    expect(sentence("token expired 12 min ago (exp)")).toBe("Token expired 12 min ago.");
    expect(sentence("token is not valid yet, nbf is in 5 min")).toBe("Token is not valid yet, nbf is in 5 min.");
  });
  it("says where a token was found", () => {
    expect(locationText({ kind: "body", path: "$.access_token" })).toBe("response body $.access_token");
    expect(locationText({ kind: "cookie", name: "jwt" })).toBe("cookie jwt");
  });
});

/** A page whose visibility the test flips; timers are the fake ones. */
function pageEnv() {
  let visible = true;
  const listeners = new Set<() => void>();
  const env: TickEnv = {
    visible: () => visible,
    onVisibility: (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    setInterval: (h, ms) => globalThis.setInterval(h, ms),
    clearInterval: (h) => globalThis.clearInterval(h as ReturnType<typeof setInterval>),
  };
  return {
    env,
    hide() {
      visible = false;
      listeners.forEach((l) => l());
    },
    show() {
      visible = true;
      listeners.forEach((l) => l());
    },
    listeners: () => listeners.size,
  };
}

describe("the countdown ticker", () => {
  it("ticks once per second, pauses in the background and stops for good", () => {
    vi.useFakeTimers();
    try {
      const page = pageEnv();
      const tick = vi.fn();
      const stop = startCountdown(tick, page.env);
      vi.advanceTimersByTime(3000);
      expect(tick).toHaveBeenCalledTimes(3);
      page.hide();
      vi.advanceTimersByTime(60_000);
      expect(tick).toHaveBeenCalledTimes(3);
      expect(vi.getTimerCount()).toBe(0);
      page.show(); // one tick at once, then once per second
      expect(tick).toHaveBeenCalledTimes(4);
      vi.advanceTimersByTime(2000);
      expect(tick).toHaveBeenCalledTimes(6);
      stop();
      vi.advanceTimersByTime(5000);
      expect(tick).toHaveBeenCalledTimes(6);
      expect(vi.getTimerCount()).toBe(0);
      expect(page.listeners()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
  it("does not start in a hidden page", () => {
    vi.useFakeTimers();
    try {
      const page = pageEnv();
      page.hide();
      const tick = vi.fn();
      const stop = startCountdown(tick, page.env);
      vi.advanceTimersByTime(5000);
      expect(tick).not.toHaveBeenCalled();
      stop();
    } finally {
      vi.useRealTimers();
    }
  });
});
