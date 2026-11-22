// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GraphQlApi` of the desktop app (src-tauri/src/grpc.rs). Wave 8, strand B.

import type { GraphQlApi } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGraphQl(call: Call): GraphQlApi {
  return {
    // Secret values come from the keychain on the Rust side, never from the webview.
    schema: (path, input, refresh) =>
      call<string>("graphql_schema", {
        path,
        input: { environment: input.environment, vars: input.vars },
        refresh,
      }),
  };
}
