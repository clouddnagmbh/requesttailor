// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { TokenInfo } from "../commands/types";
import { countdown, duration, tokenState } from "./format";

const now = Date.UTC(2026, 10, 22, 12, 0, 0);
const info = (expires_at: number | null, refreshable = false): TokenInfo => ({
  key: "k",
  label: "idp · app",
  obtained_at: now - 60_000,
  expires_at,
  refreshable,
});

describe("duration", () => {
  it("says seconds, minutes, hours and days", () => {
    expect(duration(0)).toBe("0 s");
    expect(duration(42_000)).toBe("42 s");
    expect(duration(90_000)).toBe("1 min");
    expect(duration(59 * 60_000)).toBe("59 min");
    expect(duration(2 * 3_600_000 + 5 * 60_000)).toBe("2 h 5 min");
    expect(duration(3 * 86_400_000)).toBe("3 d");
  });
});

describe("tokenState", () => {
  it("is valid with the remaining time", () => {
    expect(tokenState(info(now + 30 * 60_000), now)).toEqual({ kind: "valid", text: "valid for 30 min" });
  });
  it("is valid without expiry", () => {
    expect(tokenState(info(null), now)).toEqual({ kind: "valid", text: "no expiry given" });
  });
  it("is expired, and says whether it refreshes on the next send", () => {
    expect(tokenState(info(now - 1), now)).toEqual({ kind: "expired", text: "expired" });
    expect(tokenState(info(now - 1, true), now)).toEqual({
      kind: "expired",
      text: "expired, refreshed on the next send",
    });
  });
});

describe("countdown", () => {
  it("counts down minutes and seconds and stops at zero", () => {
    expect(countdown(600, now, now)).toBe("10:00");
    expect(countdown(600, now, now + 61_500)).toBe("8:59");
    expect(countdown(5, now, now + 10_000)).toBe("0:00");
  });
});
