// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand W: the main-area view of API notebooks (`ToolId` `notebook`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import NotebookView from "./NotebookView.svelte";

export const notebookTool: ToolDefinition | null = {
  id: "notebook",
  title: "Notebooks",
  component: NotebookView,
};
