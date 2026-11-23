// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` of the desktop app (src-tauri/src/mcp_client.rs). Wave 8, strand D.
// stdio and Streamable HTTP servers; secret values come from the keychain on the Rust side,
// never from the webview. Wave 14: resources, prompts, `rt mcp` on the open collection
// (`selfServer`) and the demo server on a local port (`localDemo`).

import type { McpClientApi, SendInput } from "@requesttailor/ui";

import type { Call } from "./backend";

/** The input without secret values (the Rust side reads them from the keychain). */
const plain = (input: SendInput) => ({ environment: input.environment, vars: input.vars });

export function tauriMcp(call: Call): McpClientApi {
  return {
    connect: (server, input) => call("mcp_connect", { server, input: plain(input) }),
    tools: (id) => call("mcp_tools", { id }),
    call: (id, tool, args) => call("mcp_call", { id, tool, args }),
    disconnect: (id) => call("mcp_disconnect", { id }),
    tests: () => call("mcp_tests"),
    getTest: (path) => call("mcp_get_test", { path }),
    saveTest: (path, test) => call("mcp_save_test", { path, test }),
    runTest: (path, input) => call("mcp_run_test", { path, input: plain(input) }),
    resources: (id) => call("mcp_resources", { id }),
    readResource: (id, uri) => call("mcp_read_resource", { id, uri }),
    prompts: (id) => call("mcp_prompts", { id }),
    getPrompt: (id, name, args) => call("mcp_get_prompt", { id, name, args }),
    selfServer: () => call("mcp_self_server"),
    localDemo: {
      status: () => call("mcp_demo_status"),
      start: () => call("mcp_demo_start"),
      stop: () => call("mcp_demo_stop"),
    },
  };
}
