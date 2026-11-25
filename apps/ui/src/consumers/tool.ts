// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand F: the main-area view of the consumer contracts (`ToolId` `consumers`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import ConsumersView from "./ConsumersView.svelte";

export const consumersTool: ToolDefinition | null = {
  id: "consumers",
  title: "Verify consumers",
  component: ConsumersView,
};
