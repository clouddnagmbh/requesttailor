// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: the working copy of a repository with 50 000 tracked files, 100 of them
// changed (`git_status_large`, perf/README.md). Interface placeholder: measures nothing, so
// the budget stays "pending" until strand B builds the measurement here (a synthetic repository
// in a temporary folder, `cargo run --release --example perf_status -p rt-git`, median of 9).

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureStatus(_context) {
  return { results: {}, details: { pending: "wave 15, strand B" } };
}
