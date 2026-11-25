// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { describe, expect, it } from "vitest";

import type { DebugState } from "../commands/types";
import { isOver, isPaused, statusLine, toggleBreakpoint } from "./model";

describe("debugger model", () => {
  it("toggles breakpoints and keeps them sorted", () => {
    let list = toggleBreakpoint([], 5);
    list = toggleBreakpoint(list, 2);
    expect(list.map((b) => b.line)).toEqual([2, 5]);
    expect(toggleBreakpoint(list, 2).map((b) => b.line)).toEqual([5]);
  });

  it("describes the state", () => {
    const paused: DebugState = { session: 1, status: "paused", pause: { line: 3, reason: "breakpoint", frames: [], scopes: [] } };
    expect(statusLine(paused)).toBe("Stopped at line 3 (breakpoint)");
    expect(isPaused(paused)).toBe(true);
    expect(isOver({ session: 1, status: "finished" })).toBe(true);
    expect(statusLine(null)).toBe("Not running");
  });
});
