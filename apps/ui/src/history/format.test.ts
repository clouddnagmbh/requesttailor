// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { HistoryEntry } from "../commands/types";
import { entryClass, statusLabel, timeAgo } from "./format";

const entry = (status: number | null): HistoryEntry => ({
  id: 1,
  at: 0,
  collection: "c",
  request_path: "a.yml",
  name: "A",
  environment: null,
  method: "GET",
  url: "http://x",
  status,
  error: status == null ? { kind: "transport", message: { kind: "timeout" } } : null,
  duration_ms: status == null ? null : 12,
  size: null,
});

describe("history format", () => {
  it("says how long ago", () => {
    const now = Date.UTC(2026, 10, 22, 12, 0, 0);
    expect(timeAgo(now, now)).toBe("just now");
    expect(timeAgo(now - 30_000, now)).toBe("just now");
    expect(timeAgo(now - 2 * 60_000, now)).toBe("2 min ago");
    expect(timeAgo(now - 59 * 60_000, now)).toBe("59 min ago");
    expect(timeAgo(now - 3 * 3_600_000, now)).toBe("3 h ago");
    expect(timeAgo(now - 24 * 3_600_000, now)).toBe("yesterday");
    expect(timeAgo(now - 4 * 86_400_000, now)).toBe("4 days ago");
    expect(timeAgo(Date.UTC(2026, 0, 2), now)).toBe("2026-01-02");
    // A clock that went backwards is "just now", not negative.
    expect(timeAgo(now + 60_000, now)).toBe("just now");
  });

  it("colours by status class", () => {
    expect(entryClass(entry(200))).toBe("ok");
    expect(entryClass(entry(302))).toBe("redirect");
    expect(entryClass(entry(404))).toBe("client-error");
    expect(entryClass(entry(503))).toBe("server-error");
    expect(entryClass(entry(null))).toBe("error");
    expect(statusLabel(entry(404))).toBe("404");
    expect(statusLabel(entry(null))).toBe("Error");
  });
});
