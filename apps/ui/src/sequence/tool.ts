// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: the main-area view of sequence diagrams (`ToolId` `sequence`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import SequenceView from "./SequenceView.svelte";

export const sequenceTool: ToolDefinition | null = {
  id: "sequence",
  title: "Sequence diagram",
  component: SequenceView,
};
