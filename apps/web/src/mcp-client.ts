// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` of the web app (wave 8, strand D), over rt-wasm's MCP client
// (crates/rt-wasm/src/mcp_js.rs). Streamable HTTP only: the browser cannot start local
// commands, so stdio servers are refused with a clear message. Requests go out like a send:
// through `fetch`, or the companion when it is on; the secret values come from the browser
// store and are masked in everything the core returns.

import {
  BackendError,
  type McpCallResult,
  type McpClientApi,
  type McpServerConfig,
  type McpServerInfo,
  type McpTest,
  type McpTestResult,
  type McpTool,
  type SendInput,
} from "@requesttailor/ui";

import { toBackendError, type CompanionCredentials, type CoreSendInput } from "./wasm";
import type { Session } from "./web-backend";

/** The MCP part of rt-wasm's `Workspace`. */
export interface CoreMcpWorkspace {
  mcpConnect(
    server: McpServerConfig,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
  ): Promise<{ id: number; info: McpServerInfo }>;
  mcpTools(id: number, companion: CompanionCredentials | null): Promise<McpTool[]>;
  mcpCall(id: number, tool: string, args: unknown, companion: CompanionCredentials | null): Promise<McpCallResult>;
  mcpDisconnect(id: number, companion: CompanionCredentials | null): Promise<void>;
  mcpTests(): string[];
  mcpGetTest(path: string): McpTest;
  mcpSaveTest(path: string | null, test: McpTest, secretValues: string[]): string;
  mcpRunTest(path: string, input: CoreSendInput, companion: CompanionCredentials | null): Promise<McpTestResult>;
}

export interface WebMcpHost {
  session(): Session;
  /** The companion's credentials while it is on, else `null` (`fetch`). */
  companion(): Promise<CompanionCredentials | null>;
  /** The core input with the secret values, as for a send. */
  coreInput(input: SendInput): Promise<CoreSendInput>;
  /** Stored values of every secret variable of the collection's environments. */
  secretValues(): Promise<string[]>;
}

export const STDIO_IN_BROWSER =
  "The web app cannot start local commands (stdio MCP servers). Connect over HTTP, or use the desktop app or `rt mcp-test`.";

async function guard<T>(f: () => Promise<T>): Promise<T> {
  try {
    return await f();
  } catch (e) {
    throw toBackendError(e);
  }
}

export function webMcp(host: WebMcpHost): McpClientApi {
  const ws = () => host.session().ws as unknown as CoreMcpWorkspace;
  return {
    connect: (server, input) =>
      guard(async () => {
        if (server.transport === "stdio") throw new BackendError("unsupported", STDIO_IN_BROWSER);
        return ws().mcpConnect(server, await host.coreInput(input), await host.companion());
      }),
    tools: (id) => guard(async () => ws().mcpTools(id, await host.companion())),
    call: (id, tool, args) => guard(async () => ws().mcpCall(id, tool, args ?? {}, await host.companion())),
    disconnect: (id) => guard(async () => ws().mcpDisconnect(id, await host.companion())),
    tests: () => guard(async () => ws().mcpTests()),
    getTest: (path) => guard(async () => ws().mcpGetTest(path)),
    saveTest: (path, test) => guard(async () => ws().mcpSaveTest(path, test, await host.secretValues())),
    runTest: (path, input) =>
      guard(async () => ws().mcpRunTest(path, await host.coreInput(input), await host.companion())),
  };
}
