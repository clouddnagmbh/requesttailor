// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `McpClientApi` of the desktop app (src-tauri/src/mcp_client.rs). Wave 8, strand D.
// stdio and Streamable HTTP servers; secret values come from the keychain on the Rust side,
// never from the webview.

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
  };
}
