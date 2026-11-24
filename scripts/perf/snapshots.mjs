// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand B: 1000 stored responses against their snapshots (release rt-app) (perf/README.md). Interface placeholder:
// measures nothing, so the budget stays "pending" until strand B builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureSnapshots(_context) {
  return { results: {}, details: { pending: "wave 17, strand B" } };
}
