// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18 interface: parts that want a word before a request goes out (a token that has
// expired, a rate limit that is used up). `AppState.sendGate` stays the OpenAPI run wizard's
// (`openapi/gate.ts`, behaviour unchanged); these gates run after it, in order, and the first
// `false` cancels the send. An error lets the send through, like the other gates.

import type { AppState } from "../state/app.svelte";

/** `false` cancels the send of `path`. */
export type SendGateParticipant = (app: AppState, path: string) => Promise<boolean>;

const participants: SendGateParticipant[] = [];

/** Strands add their gate here (once, at start). */
export function addSendGate(gate: SendGateParticipant): void {
  participants.push(gate);
}

/** Runs the gates; `false` if one of them cancelled. */
export async function runSendGates(app: AppState, path: string): Promise<boolean> {
  for (const gate of participants) {
    if (!(await gate(app, path).catch(() => true))) return false;
  }
  return true;
}
