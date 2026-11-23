// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `VariablesApi` of the web app (wave 14, variable completion), over rt-wasm's `Workspace`
// (crates/rt-wasm/src/variables_js.rs). The input is built like for `preview`: the session's
// variables in the runtime layer, the secret values only to mask; none is ever returned.

import type { SendInput, VariableScope, Variable, VariablesApi } from "@requesttailor/ui";

import { toBackendError, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** The variables part of rt-wasm's `Workspace`. */
export interface CoreVariablesWorkspace {
  variables(path: string, input: CoreSendInput): VariableScope;
  defineVariable(folder: string, variable: Variable): void;
}

export function webVariables(
  session: () => Session,
  coreInput: (s: Session, input: SendInput) => Promise<CoreSendInput>,
): VariablesApi {
  const ws = (s: Session) => s.ws as unknown as CoreVariablesWorkspace;
  return {
    scope: async (path, input) => {
      try {
        const s = session();
        return ws(s).variables(path, await coreInput(s, input));
      } catch (e) {
        throw toBackendError(e);
      }
    },
    define: async (folder, variable) => {
      try {
        ws(session()).defineVariable(folder, variable);
      } catch (e) {
        throw toBackendError(e);
      }
    },
  };
}
