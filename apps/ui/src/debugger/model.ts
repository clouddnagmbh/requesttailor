// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: what the debugger view derives from the core's state (pure, tested).

import type { Breakpoint, DebugState, DebugValue, ScriptPhase } from "../commands/types";

export const PHASES: ScriptPhase[] = ["before-request", "after-response", "tests"];

/** Phases that read a response need a kept one. */
export const needsResponse = (phase: ScriptPhase) => phase !== "before-request";

/** Adds or removes the breakpoint at `line`. */
export function toggleBreakpoint(list: Breakpoint[], line: number): Breakpoint[] {
  return list.some((b) => b.line === line)
    ? list.filter((b) => b.line !== line)
    : [...list, { line }].sort((a, b) => a.line - b.line);
}

/** Source lines, 1-based. */
export function linesOf(code: string): string[] {
  return code.split("\n");
}

export const isPaused = (s: DebugState | null): boolean => s?.status === "paused";
export const isOver = (s: DebugState | null): boolean => s?.status === "finished" || s?.status === "aborted";

/** One line of status for the header. */
export function statusLine(s: DebugState | null): string {
  if (!s) return "Not running";
  switch (s.status) {
    case "running":
      return "Running";
    case "paused":
      return `Stopped at line ${s.pause?.line ?? "?"} (${s.pause?.reason ?? "step"})`;
    case "finished":
      return s.error ? `Failed: ${s.error}` : "Finished";
    case "aborted":
      return "Aborted";
  }
}

export function showValue(v: DebugValue): string {
  return `${v.name} = ${v.preview}`;
}
