// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C: the main-area view of the test-data journal (`ToolId` `journal`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import JournalView from "./JournalView.svelte";

export const journalTool: ToolDefinition | null = {
  id: "journal",
  title: "Test-data journal",
  component: JournalView,
};
