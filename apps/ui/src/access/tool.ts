// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand D: the main-area view of the two-user access check (`ToolId` `access`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import AccessView from "./AccessView.svelte";

export const accessTool: ToolDefinition | null = {
  id: "access",
  title: "Check access",
  component: AccessView,
};
