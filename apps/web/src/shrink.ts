// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `ShrinkApi` of the web app (wave 19, strand E), over rt-wasm's `Workspace.shrink`
// (crates/rt-wasm/src/shrink_js.rs). Every probe goes out like a single send (companion when it
// is on, the same services, so the send guard answers for each one), with the environment's
// secret values that never leave the core. One search at a time.

import type { SendInput, ShrinkApi, ShrinkPredicate, ShrinkRequest, ShrinkResult } from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { CompanionCredentials, CoreSendInput } from "./wasm";
import { toBackendError } from "./wasm";
import type { CoreSendServices } from "./send-services";
import type { Session } from "./web-backend";

/** The minimal-case part of rt-wasm's `Workspace`. */
export interface CoreShrinkWorkspace {
  shrink(
    request: ShrinkRequest,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    cancelled: () => boolean,
  ): Promise<ShrinkResult>;
  shrinkPredicate(responseId: number): ShrinkPredicate;
}

export interface ShrinkHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
}

export function webShrink(host: ShrinkHost): ShrinkApi {
  let busy = false;
  let cancelled = false;
  return {
    run: async (request) => {
      if (busy) throw toBackendError(new Error("A minimal-case search is already in progress."));
      busy = true;
      cancelled = false;
      try {
        const ws = host.session().ws as unknown as CoreShrinkWorkspace;
        const context = await host.sendContext({ environment: request.environment ?? null, vars: [] });
        return await ws.shrink(request, context.input, context.companion ?? null, context.services, () => cancelled);
      } catch (e) {
        throw toBackendError(e);
      } finally {
        busy = false;
      }
    },
    cancel: async () => {
      cancelled = true;
    },
    predicate: async (path, responseId) => {
      try {
        if (responseId === null) throw new Error(`${path} has no answer yet: send it first.`);
        return (host.session().ws as unknown as CoreShrinkWorkspace).shrinkPredicate(responseId);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
