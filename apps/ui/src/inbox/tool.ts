// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E: the main-area view of the webhook inbox (`ToolId` `inbox`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import InboxView from "./InboxView.svelte";

export const inboxTool: ToolDefinition | null = { id: "inbox", title: "Webhook inbox", component: InboxView };
