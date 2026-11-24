// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: `env_compare` (perf/README.md). Interface placeholder: measures nothing, so
// the budget stays "pending" until strand A builds the measurement here.

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureEnvCompare(_context) {
  return { results: {}, details: { pending: "wave 18, strand A" } };
}
