// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `GrpcApi` of the web app (wave 8, strand B), over rt-wasm's `Workspace.grpcServices` and
// `Workspace.grpcTemplate` (crates/rt-wasm/src/grpc_js.rs). Descriptors come from the proto
// files of the opened folder; server reflection goes through the companion's stream relay
// (without the companion it rejects with a message that says so).

import type { GrpcApi, GrpcService, SendInput } from "@requesttailor/ui";

import type { RunContext } from "./runner";
import {
  toBackendError,
  type CompanionCredentials,
  type CoreSendInput,
} from "./wasm";
import type { Session } from "./web-backend";

/** The gRPC part of rt-wasm's `Workspace`. */
export interface CoreGrpcWorkspace {
  grpcServices(
    path: string,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
  ): Promise<GrpcService[]>;
  grpcTemplate(
    path: string,
    method: string,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
  ): Promise<string>;
}

/** What the gRPC and GraphQL parts need from the web backend. */
export interface WebProtocolHost {
  session(): Session;
  sendContext(input: SendInput): Promise<RunContext>;
}

export async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

export function webGrpc(host: WebProtocolHost): GrpcApi {
  const ws = () => host.session().ws as unknown as CoreGrpcWorkspace;
  return {
    services: (path, input) =>
      guard(async () => {
        const c = await host.sendContext(input);
        return ws().grpcServices(path, c.input, c.companion);
      }),
    template: (path, method, input) =>
      guard(async () => {
        const c = await host.sendContext(input);
        return ws().grpcTemplate(path, method, c.input, c.companion);
      }),
  };
}
