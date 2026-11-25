// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I: the main-area view of the plugin catalogue (`ToolId` `catalogue`).

import CatalogueView from "../plugins/catalogue/CatalogueView.svelte";
import type { ToolDefinition } from "../shell/tool-host.svelte";

export const catalogueTool: ToolDefinition | null = { id: "catalogue", title: "Plugin catalogue", component: CatalogueView };
