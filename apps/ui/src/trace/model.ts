// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand A: small pure helpers of the trace parts of the response area.

/** "4bf92f3577b34da6a3ce929d0e0e4736" → "4bf9…" (the full id is in the tooltip and on copy). */
export function shortTraceId(id: string): string {
  return id.length > 4 ? `${id.slice(0, 4)}…` : id;
}
