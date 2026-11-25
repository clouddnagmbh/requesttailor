// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `NotebookApi` of the web app (wave 19, strand W), over rt-wasm's `Workspace.notebook*`
// (crates/rt-wasm/src/notebook_js.rs). A notebook is a companion file of the open collection,
// staged until save. Running sends the blocks like a single send (companion when it is on, the
// same services, so the send guard answers for each one) with the environment's secret values,
// which never leave the core. One run at a time. The web keeps no audit log; outputs are
// returned and never stored.

import type {
  Notebook,
  NotebookApi,
  NotebookEntry,
  NotebookOutcome,
  NotebookRun,
  SendInput,
} from "@requesttailor/ui";

import type { RunContext } from "./runner";
import type { CompanionCredentials, CoreSendInput } from "./wasm";
import { toBackendError } from "./wasm";
import type { CoreSendServices } from "./send-services";
import type { Session } from "./web-backend";

/** The notebook part of rt-wasm's `Workspace`. */
export interface CoreNotebookWorkspace {
  notebookList(): NotebookEntry[];
  notebookRead(path: string): Notebook;
  notebookStage(path: string, notebook: Notebook): void;
  notebookRun(
    run: NotebookRun,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
    cancelled: () => boolean,
  ): Promise<NotebookOutcome>;
}

export interface NotebookHost {
  session(): Session;
  /** What a send needs (secret values, companion, services), as for the runner. */
  sendContext(input: SendInput): Promise<RunContext>;
}

export function webNotebook(host: NotebookHost): NotebookApi {
  let busy = false;
  const ws = () => host.session().ws as unknown as CoreNotebookWorkspace;
  const sync = <T>(f: () => T): Promise<T> => {
    try {
      return Promise.resolve(f());
    } catch (e) {
      return Promise.reject(toBackendError(e));
    }
  };
  return {
    list: () => sync(() => ws().notebookList()),
    read: (path) => sync(() => ws().notebookRead(path)),
    stage: (path, notebook) => sync(() => ws().notebookStage(path, notebook)),
    run: async (run) => {
      if (busy) throw toBackendError(new Error("A notebook is already running."));
      busy = true;
      try {
        const context = await host.sendContext({ environment: run.environment ?? null, vars: [] });
        return await ws().notebookRun(run, context.input, context.companion, context.services, () => false);
      } catch (e) {
        throw toBackendError(e);
      } finally {
        busy = false;
      }
    },
  };
}
