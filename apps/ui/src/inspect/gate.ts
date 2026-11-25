// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand B: the word before a request goes out with a token that has expired, is not
// valid yet or runs out in under 30 seconds ("Token expired 12 min ago. Send anyway?"). It is a
// participant of `shell/send-gates.ts`: it asks the core for the tokens of the *resolved* request
// (variables, environment, secrets resolved on the Rust side), shows a calm dialog only when
// there is something to say, and lets the send through on any error. Enter sends, Escape
// cancels; "don't warn again" is a setting of this device. The normal path (nothing to say)
// costs no key press.

import { addSendGate } from "../shell/send-gates";
import type { AppState } from "../state/app.svelte";
import { locationText, sendWarning, sentence } from "./model";
import { warningEnabled } from "./settings";
import TokenWarning from "./TokenWarning.svelte";

/** What the dialog answers. */
export type TokenAnswer = "send" | "new-token" | undefined;

/** Asks the user if the resolved request of `path` carries a token that is not good; resolves
 *  `false` to cancel the send. */
export async function tokenGate(app: AppState, path: string): Promise<boolean> {
  if (!warningEnabled()) return true;
  let found;
  try {
    found = await app.backend.inspect.tokens({ path, environment: app.environment });
  } catch {
    return true;
  }
  const hit = sendWarning(found);
  if (!hit) return true;
  let oauth = false;
  try {
    oauth = (await app.backend.getRequest(path)).auth.type === "oauth2";
  } catch {
    // The offer of a new token is only an offer.
  }
  const answer = (await app.showDialog({
    title: "Send with this token?",
    component: TokenWarning,
    width: 460,
    props: { text: sentence(hit.warning.text), where: locationText(hit.where), oauth },
  })) as TokenAnswer;
  if (answer === "new-token") {
    try {
      await app.backend.auth.fetchToken(path, { environment: app.environment, vars: [] });
    } catch {
      // The send goes on; a failed flow shows in the report like any auth error.
    }
    return true;
  }
  return answer === "send";
}

let registered = false;

/** Adds the gate to `shell/send-gates.ts` once. */
export function registerTokenGate(): void {
  if (registered) return;
  registered = true;
  addSendGate(tokenGate);
}
