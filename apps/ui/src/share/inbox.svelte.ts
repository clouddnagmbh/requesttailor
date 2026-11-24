// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The share inbox (wave 17, strand C): a link that reached the app from outside (the fragment
// of the web app's address) waits here until the user opens it, and what a shared request
// still needs after it was opened. Nothing is read from the network and nothing runs.

import type { Backend } from "../commands/backend";
import type { SharePayload } from "../commands/types";

/** A link that waits to be opened. */
export interface SharedOffer {
  /** The text as it came (a fragment, a link or an `rt:` payload). */
  input: string;
  payload: SharePayload;
}

/** What the user is told after a shared request was opened as a draft. */
export interface SharedOpened {
  name: string;
  path: string;
  /** Variables the request uses that the recipient has to define. */
  needs: string[];
  /** The part of `needs` that must be secret variables. */
  secrets: string[];
}

export const sharedInbox = $state<{ offer: SharedOffer | null; opened: SharedOpened | null; error: string | null }>({
  offer: null,
  opened: null,
  error: null,
});

/** Puts a link into the inbox; the banner shows it. Reads it in the core, makes no request. */
export async function offerSharedLink(backend: Pick<Backend, "share">, input: string): Promise<boolean> {
  try {
    sharedInbox.offer = { input, payload: await backend.share.open(input) };
    sharedInbox.error = null;
    return true;
  } catch (e) {
    sharedInbox.offer = null;
    sharedInbox.error = e instanceof Error ? e.message : String(e);
    return false;
  }
}

export function clearSharedInbox(): void {
  sharedInbox.offer = null;
  sharedInbox.opened = null;
  sharedInbox.error = null;
}
