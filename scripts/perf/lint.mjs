// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand F: `lint_1000` (perf/README.md). Interface placeholder: measures nothing, so
// the budget stays "pending" until strand F builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureLint(_context) {
  return { results: {}, details: { pending: "wave 18, strand F" } };
}
