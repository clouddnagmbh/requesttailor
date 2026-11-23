// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` of the web app (wave 8, strand D), over rt-wasm's MCP client
// (crates/rt-wasm/src/mcp_js.rs). Streamable HTTP only: the browser cannot start local
// commands, so stdio servers are refused with a clear message. Requests go out like a send:
// through `fetch`, or the companion when it is on; the secret values come from the browser
// store and are masked in everything the core returns. The demo server (`rt-demo://mcp`)
// runs inside the core, in the browser: nothing to install. `rt mcp` and a demo on a local
// port need a native process, so `selfServer` explains and `localDemo` is null.

import {
  BackendError,
  type McpCallResult,
  type McpClientApi,
  type McpPrompt,
  type McpPromptResult,
  type McpResource,
  type McpResourceContents,
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
  mcpResources(id: number, companion: CompanionCredentials | null): Promise<McpResource[]>;
  mcpReadResource(id: number, uri: string, companion: CompanionCredentials | null): Promise<McpResourceContents[]>;
  mcpPrompts(id: number, companion: CompanionCredentials | null): Promise<McpPrompt[]>;
  mcpGetPrompt(id: number, name: string, args: [string, string][], companion: CompanionCredentials | null): Promise<McpPromptResult>;
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

export const SELF_IN_BROWSER =
  "The browser cannot start rt mcp. In a terminal, run `rt mcp` in the collection folder and add it to your AI assistant (for example `claude mcp add requesttailor -- rt mcp /path/to/collection`), or use the desktop app to try it here.";

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
    resources: (id) => guard(async () => ws().mcpResources(id, await host.companion())),
    readResource: (id, uri) => guard(async () => ws().mcpReadResource(id, uri, await host.companion())),
    prompts: (id) => guard(async () => ws().mcpPrompts(id, await host.companion())),
    getPrompt: (id, name, args) => guard(async () => ws().mcpGetPrompt(id, name, args, await host.companion())),
    selfServer: async () => ({ server: null, hint: SELF_IN_BROWSER }),
    localDemo: null,
  };
}
