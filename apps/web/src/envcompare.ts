// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `EnvCompareApi` of the web app (wave 18, strand A), over rt-wasm's `Workspace.compareEnvironments`
// (crates/rt-wasm/src/envcompare_js.rs). Every request goes out like a single send, once per
// environment, with that environment's secret values, through the companion when it is on, with
// the same services (cookies, tokens and session variables are scoped by the environment).

import type { EnvCompareApi, EnvCompareRequest, EnvComparison, SendInput } from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { CompanionCredentials, CoreSendInput } from "./wasm";
import { toBackendError } from "./wasm";
import type { CoreSendServices } from "./send-services";
import type { Session } from "./web-backend";

/** The environment-comparison part of rt-wasm's `Workspace`. */
export interface CoreEnvCompareWorkspace {
  compareEnvironments(
    request: EnvCompareRequest,
    inputs: CoreSendInput[],
    companion: CompanionCredentials | null,
    services: (CoreSendServices | null)[],
    cancelled: () => boolean,
  ): Promise<EnvComparison>;
}

export interface EnvCompareHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
}

export function webEnvCompare(host: EnvCompareHost): EnvCompareApi {
  // One comparison at a time.
  let busy = false;
  return {
    run: async (request) => {
      if (busy) throw toBackendError(new Error("An environment comparison is already in progress."));
      busy = true;
      try {
        const ws = host.session().ws as unknown as CoreEnvCompareWorkspace;
        const contexts: RunContext[] = [];
        for (const environment of request.environments) {
          contexts.push(await host.sendContext({ environment, vars: [] }));
        }
        return await ws.compareEnvironments(
          request,
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
