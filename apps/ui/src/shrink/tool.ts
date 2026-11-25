// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand E: the main-area view of the minimal failing request (`ToolId` `shrink`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import ShrinkView from "./ShrinkView.svelte";

export const shrinkTool: ToolDefinition | null = {
  id: "shrink",
  title: "Minimal failing request",
  component: ShrinkView,
};
