// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: the main-area view of the environment comparison (`ToolId` `env-compare`).

import type { ToolDefinition } from "../shell/tool-host.svelte";
import EnvCompareView from "./EnvCompareView.svelte";

export const envcompareTool: ToolDefinition | null = {
  id: "env-compare",
  title: "Compare environments",
  component: EnvCompareView,
};
