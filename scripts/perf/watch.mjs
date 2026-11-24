// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: the resend of rt send --watch (perf/README.md). Interface placeholder:
// measures nothing, so the budget stays "pending" until strand D builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureWatch(_context) {
  return { results: {}, details: { pending: "wave 17, strand D" } };
}
