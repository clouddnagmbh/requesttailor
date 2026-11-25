// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand F: the main-area view of the lint (`ToolId` `lint`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import LintView from "./LintView.svelte";

export const lintTool: ToolDefinition | null = { id: "lint", title: "Lint", component: LintView };
