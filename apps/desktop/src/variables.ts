// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `VariablesApi` of the desktop app (src-tauri/src/variables.rs, wave 14): the variables a
// request sees for the `{{` completion list, and new collection or folder variables.

import type { SendInput, VariablesApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriVariables(call: Call, input: (i: SendInput) => unknown): VariablesApi {
  return {
    scope: (path, i) => call("variables_scope", { path, input: input(i) }),
    define: (folder, variable) => call("variables_define", { folder, variable }),
  };
}
