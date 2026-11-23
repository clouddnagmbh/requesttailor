// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: the commit graph of a repository with 100 000 commits
// (`git_graph_large`, perf/README.md). Interface placeholder: measures nothing, so the budget
// stays "pending" until strand C builds the measurement here (a synthetic repository, the first
// page natively with `cargo run --release --example perf_graph -p rt-git`, plus the first rows
// in the view; median of 5).

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureGraphLarge(_context) {
  return { results: {}, details: { pending: "wave 15, strand C" } };
}
