// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `CompanionApi` of the web app (wave 13), over rt-wasm's `Workspace` (crates/rt-wasm/src/
// companions_js.rs). Staged in the core workspace; `save` writes them like request edits.

import type { CompanionApi } from "@requesttailor/ui";

import { toBackendError } from "./wasm";
import type { Session } from "./web-backend";

/** The companion part of rt-wasm's `Workspace`. */
export interface CoreCompanionWorkspace {
  companionList(prefix: string): string[];
  companionRead(path: string): string | undefined;
  companionStage(path: string, text: string): void;
}

export function webCompanions(session: () => Session): CompanionApi {
  const ws = () => session().ws as unknown as CoreCompanionWorkspace;
  const sync = <T>(f: () => T): Promise<T> => {
    try {
      return Promise.resolve(f());
    } catch (e) {
      return Promise.reject(toBackendError(e));
    }
  };
  return {
    list: (prefix) => sync(() => ws().companionList(prefix)),
    read: (path) => sync(() => ws().companionRead(path) ?? null),
    stage: (path, text) => sync(() => ws().companionStage(path, text)),
  };
}
