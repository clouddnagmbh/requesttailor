// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The guided first run of the MCP view (wave 14): connect to the demo server, see its tools,
// call fake_weather with the form, save the call as a test, run all tests green. Pure, so
// the steps are unit-tested; McpState records progress, the sidebar shows the checklist and
// the panel a coachmark for the next step. Hidden per device once dismissed or finished.

import { MCP_DEMO_URL } from "../commands/backend";
import type { McpServerConfig } from "../commands/types";

export type GuideStep = "connect" | "list" | "call" | "save" | "run";

export type GuideProgress = Record<GuideStep, boolean>;

export interface GuideStepInfo {
  id: GuideStep;
  title: string;
  /** What to do, shown in the coachmark while this is the next step. */
  hint: string;
}

export const GUIDE_STEPS: GuideStepInfo[] = [
  { id: "connect", title: "Connect to the demo server", hint: "Choose “Try the demo server”. It runs inside RequestTailor, offline." },
  { id: "list", title: "See its tools", hint: "The server lists its tools. Each has a description and a schema for its arguments." },
  { id: "call", title: "Call fake_weather", hint: "Pick fake_weather, type a city into the form and choose “Call tool”." },
  { id: "save", title: "Save the call as a test", hint: "Choose “Save as test…”: the suggested checks come from the result. Save writes it next to your collection." },
  { id: "run", title: "Run all tests", hint: "Choose “Run all” in the sidebar. Green means the server still answers as saved; rt mcp-test does the same in CI." },
];

/** The demo tool the guide calls. */
export const GUIDE_TOOL = "fake_weather";

export const noProgress = (): GuideProgress => ({ connect: false, list: false, call: false, save: false, run: false });

/** The first step not done yet, or `null` when all are done. Steps count only in order. */
export function nextStep(p: GuideProgress): GuideStep | null {
  return GUIDE_STEPS.find((s) => !p[s.id])?.id ?? null;
}

export function doneCount(p: GuideProgress): number {
  const next = nextStep(p);
  return next === null ? GUIDE_STEPS.length : GUIDE_STEPS.findIndex((s) => s.id === next);
}

/** Whether `server` is the built-in demo server. */
export function isDemo(server: McpServerConfig): boolean {
  return server.transport === "http" && server.url.trim().replace(/\/$/, "") === MCP_DEMO_URL;
}

/** Marks `step` done if the steps before it are (a later step alone does not count). */
export function advance(p: GuideProgress, step: GuideStep): GuideProgress {
  const at = GUIDE_STEPS.findIndex((s) => s.id === step);
  if (GUIDE_STEPS.slice(0, at).some((s) => !p[s.id])) return p;
  return { ...p, [step]: true };
}
