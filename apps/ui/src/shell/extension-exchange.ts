// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AppExtension.onExchange` (wave 17): after every send of the app, each extension that has it
// hears what went out and what came back. An event, not a reactive scope: the call is
// synchronous and quick, an extension that throws is logged and never stops the others or the
// send. The collection's `observe.enabled: false` stops the event before it gets here
// (`AppState.onSent`).

import type { AppExtension, ExchangeEvent, ExtensionContext } from "../extensions";
import type { AppState, SentExchange } from "../state/app.svelte";

/** Hands `event` to every extension with an `onExchange`. */
export function deliverExchange(extensions: readonly AppExtension[], ctx: ExtensionContext, event: ExchangeEvent): void {
  for (const x of extensions) {
    if (!x.onExchange) continue;
    try {
      x.onExchange(ctx, event);
    } catch (e) {
      console.warn(`RequestTailor: extension ${x.id} failed in onExchange:`, e);
    }
  }
}

/** Connects the extensions to the sends of `app`; returns the disconnect. Nothing is
 *  connected (and nothing is read after a send) when no extension wants the event. */
export function connectExchange(app: AppState, extensions: readonly AppExtension[], ctx: ExtensionContext): () => void {
  if (!extensions.some((x) => x.onExchange)) return () => {};
  return app.onSent((sent: SentExchange) => deliverExchange(extensions, ctx, sent));
}
