// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RunnerApi` of the web app (wave 7, strand D owns this file), over rt-wasm's runner.
// Interface stub: an empty plan and an empty run.

import type { RunnerApi, RunSummary } from "@requesttailor/ui";

import type { WebHost } from "./web-backend";

function emptySummary(): RunSummary {
  return {
    requests: 0,
    passed: 0,
    failed: 0,
    skipped: 0,
    tests_passed: 0,
    tests_failed: 0,
    assertions_passed: 0,
    assertions_failed: 0,
    duration_ms: 0,
    stopped: false,
  };
}

export function webRunner(_host: Pick<WebHost, "session">): RunnerApi {
  return {
    plan: async () => [],
    parseData: async () => [],
    run: async (_plan, _input, onEvent) => {
      const summary = emptySummary();
      onEvent({ kind: "started", total: 0 });
      onEvent({ kind: "finished", summary });
      return summary;
    },
    cancel: async () => {},
  };
}
