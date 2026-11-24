// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand G: `view_apply` (perf/README.md). Interface placeholder: measures nothing, so
// the budget stays "pending" until strand G builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureViews(_context) {
  return { results: {}, details: { pending: "wave 18, strand G" } };
}
