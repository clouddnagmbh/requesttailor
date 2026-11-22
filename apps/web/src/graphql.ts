// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GraphQlApi` of the web app (wave 8, strand B), over rt-wasm's `Workspace.graphqlSchema`
// (crates/rt-wasm/src/grpc_js.rs): the introspection query goes out like a send (`fetch`, or
// the companion when it is on, with the send services); the SDL is cached in the core.

import type { GraphQlApi } from "@requesttailor/ui";

import { guard, type WebProtocolHost } from "./grpc";
import type { CoreSendServices } from "./send-services";
import type { CompanionCredentials, CoreSendInput } from "./wasm";

/** The GraphQL part of rt-wasm's `Workspace`. */
export interface CoreGraphQlWorkspace {
  graphqlSchema(
    path: string,
    input: CoreSendInput,
    refresh: boolean,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
  ): Promise<string>;
}

export function webGraphQl(host: WebProtocolHost): GraphQlApi {
  return {
    schema: (path, input, refresh) =>
      guard(async () => {
        const ws = host.session().ws as unknown as CoreGraphQlWorkspace;
        const context = await host.sendContext(input);
        return ws.graphqlSchema(path, context.input, refresh, context.companion, context.services);
      }),
  };
}
