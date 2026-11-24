// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C: the share link of every request of the Bruno corpus (perf/README.md). Interface placeholder:
// measures nothing, so the budget stays "pending" until strand C builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureShare(_context) {
  return { results: {}, details: { pending: "wave 17, strand C" } };
}
