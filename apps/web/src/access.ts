// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `AccessApi` of the web app (wave 19, strand D), over rt-wasm's `Workspace.accessCheck`
// (crates/rt-wasm/src/access_js.rs). Both requests go out like a single send (companion when it
// is on, the same services, so the send guard answers for each one), each with its own
// environment's secret values, which never leave the core. One check at a time. The web keeps
// no audit log.

import type { AccessApi, AccessCheck, AccessOptions, AccessProfile, SendInput } from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { CompanionCredentials, CoreSendInput } from "./wasm";
import { toBackendError } from "./wasm";
import type { CoreSendServices } from "./send-services";
import type { Session } from "./web-backend";

/** The access-check part of rt-wasm's `Workspace`. */
export interface CoreAccessWorkspace {
  accessCheck(
    path: string,
    profile: AccessProfile,
    options: AccessOptions,
    inputs: CoreSendInput[],
    companion: CompanionCredentials | null,
    services: (CoreSendServices | null)[],
    cancelled: () => boolean,
  ): Promise<AccessCheck>;
}

export interface AccessHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
}

export function webAccess(host: AccessHost): AccessApi {
  let busy = false;
  return {
    check: async (path, profile, options) => {
      if (busy) throw toBackendError(new Error("An access check is already in progress."));
      busy = true;
      try {
        const ws = host.session().ws as unknown as CoreAccessWorkspace;
        const contexts: RunContext[] = [];
        for (const environment of [profile.a, profile.b]) {
          contexts.push(await host.sendContext({ environment, vars: [] }));
        }
        return await ws.accessCheck(
          path,
          profile,
          options,
          contexts.map((c) => c.input),
          contexts[0]?.companion ?? null,
          contexts.map((c) => c.services),
          () => false,
        );
      } catch (e) {
        throw toBackendError(e);
      } finally {
        busy = false;
      }
    },
  };
}
