// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 8 interface: placeholder APIs that reject with `unsupported` until a strand replaces
// them in its own file (mock-*.ts here, the shells' own files in apps/web and apps/desktop).

import type {
  AgentsApi,
  AiApi,
  GraphQlApi,
  GrpcApi,
  McpClientApi,
  StreamsApi,
} from "./backend";
import { BackendError } from "./backend";

function notYet(feature: string): () => Promise<never> {
  return () =>
    Promise.reject(
      new BackendError("unsupported", `${feature} arrives in wave 8`),
    );
}

export function stubStreams(): StreamsApi {
  const no = notYet("WebSocket, SSE and gRPC sessions");
  return { open: no, send: no, finish: no, close: no, log: no, release: no };
}

export function stubGrpc(): GrpcApi {
  const no = notYet("gRPC");
  return { services: no, template: no };
}

export function stubGraphQl(): GraphQlApi {
  return { schema: notYet("The GraphQL schema browser") };
}

export function stubAgents(): AgentsApi {
  const no = notYet("Agent settings");
  return {
    policy: async () => ({ access: "read", environments: [] }),
    setPolicy: no,
    pending: async () => [],
    decide: no,
    onPending: () => () => {},
    audit: async () => [],
  };
}

export function stubAi(): AiApi {
  const no = notYet("The AI assistant");
  return {
    config: async () => null,
    setConfig: no,
    hasKey: async () => false,
    setKey: no,
    deleteKey: no,
    ask: no,
  };
}

export function stubMcp(): McpClientApi {
  const no = notYet("The MCP client");
  return {
    connect: no,
    tools: no,
    call: no,
    disconnect: no,
    tests: async () => [],
    getTest: no,
    saveTest: no,
    runTest: no,
  };
}
