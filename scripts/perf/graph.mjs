// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: measures the commit graph (budgets `git_graph`: first rows of a repository
// with 10 000 commits visible, ms; `git_graph_frame`: p95 frame time while scrolling it, ms).
// Placeholder until strand F: measures nothing, so neither budget is in perf/budgets.toml yet.

/** Returns `{ results: { git_graph, git_graph_frame }, details }`. */
export async function measureGraph(ctx) {
  void ctx;
  return { results: {}, details: { pending: "wave 14, strand F" } };
}
