// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: the main-area view of the script debugger (`ToolId` `debugger`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import DebuggerView from "./DebuggerView.svelte";

export const debuggerTool: ToolDefinition | null = {
  id: "debugger",
  title: "Script debugger",
  component: DebuggerView,
};
