// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GrpcApi` of the desktop app (src-tauri/src/grpc.rs). Wave 8, strand B.

import type { GrpcApi, GrpcService } from "@requesttailor/ui";

import type { Call } from "./backend";

export function tauriGrpc(call: Call): GrpcApi {
  // Secret values come from the keychain on the Rust side, never from the webview.
  return {
    services: (path, input) =>
      call<GrpcService[]>("grpc_services", {
        path,
        input: { environment: input.environment, vars: input.vars },
      }),
    template: (path, method, input) =>
      call<string>("grpc_template", {
        path,
        method,
        input: { environment: input.environment, vars: input.vars },
      }),
  };
}
