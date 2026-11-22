// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RunnerApi` in the mock backend (wave 7, strand D owns this file). Interface stub: an empty
// plan and an empty run.

import type { RunnerApi } from "./backend";
import type { RunSummary } from "./types";

export function emptySummary(): RunSummary {
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

export function mockRunner(): RunnerApi {
  return {
    async plan() {
      return [];
    },
    async parseData() {
      return [];
    },
    async run(_plan, _input, onEvent) {
      const summary = emptySummary();
      onEvent({ kind: "started", total: 0 });
      onEvent({ kind: "finished", summary });
      return summary;
    },
    async cancel() {},
  };
}
