// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `RunnerApi` of the web app (wave 7, strand D owns this file), over rt-wasm's runner
// (crates/rt-wasm/src/runner_js.rs): `Workspace.runPlan`, `Workspace.run` and `parseData`.
// Every request of a run goes out like a single send: with the secret values, through the
// companion when it is on, with the same services.

import {
  BackendError,
  type DataRow,
  type RunEvent,
  type RunnerApi,
  type RunPlan,
  type RunSummary,
  type SendInput,
} from "@requesttailor/ui";

import type { CoreSendServices } from "./send-services";
import { toBackendError, type CompanionCredentials, type CoreModule, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** The runner's part of rt-wasm's `Workspace`. */
export interface CoreRunnerWorkspace {
  runPlan(folder: string | null | undefined): string[];
  run(
    plan: RunPlan,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    onEvent: (event: RunEvent) => void,
    cancelled: () => boolean,
  ): Promise<RunSummary>;
}

/** The runner's part of the rt-wasm module. */
export interface CoreRunnerModule {
  parseData(name: string, data: Uint8Array): DataRow[];
}

/** What a send needs besides the plan: the core input with secret values, the companion
 *  (or `null` for `fetch`) and the services, exactly as `WebBackend.send` gets them. */
export interface RunContext {
  input: CoreSendInput;
  companion: CompanionCredentials | null;
  services: CoreSendServices | null;
}

export interface WebRunnerHost {
  session(): Session;
  loadCore(): Promise<CoreModule>;
  sendContext(input: SendInput): Promise<RunContext>;
}

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

function workspace(host: WebRunnerHost): CoreRunnerWorkspace {
  return host.session().ws as unknown as CoreRunnerWorkspace;
}

export function webRunner(host: WebRunnerHost): RunnerApi {
  // One run at a time; `cancel` flips the flag the core polls.
  let active: { cancelled: boolean } | null = null;
  return {
    plan: (folder) => guard(async () => workspace(host).runPlan(folder)),
    parseData: (file) =>
      guard(async () => {
        const core = (await host.loadCore()) as unknown as CoreRunnerModule;
        return core.parseData(file.name, file.data);
      }),
    run: (plan, input, onEvent) =>
      guard(async () => {
        if (active) throw new BackendError("shell", "A run is already in progress.");
        const token = { cancelled: false };
        active = token;
        try {
          const ws = workspace(host);
          const context = await host.sendContext(input);
          return await ws.run(plan, context.input, context.companion, context.services, onEvent, () => token.cancelled);
        } finally {
          active = null;
        }
      }),
    cancel: async () => {
      if (active) active.cancelled = true;
    },
  };
}
