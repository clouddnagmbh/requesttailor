// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand H: measures the comparison view (budget `compare_view`: a comparison of 1000
// requests, 100 changed, visible, ms). Placeholder until strand H: measures nothing, so the budget
// is not in perf/budgets.toml yet.

/** Returns `{ results: { compare_view }, details }`. */
export async function measureCompare(ctx) {
  void ctx;
  return { results: {}, details: { pending: "wave 14, strand H" } };
}
