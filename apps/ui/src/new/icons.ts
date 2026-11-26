// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The icon set of the "New…" dialog and of node chips (wave 14, strand B draws them). Extensions
// name an icon (`ExtensionNewItem.icon`, `ExtensionNodeAction.icon`); unknown names get the
// generic tile icon.

import "../ui";

export const NEW_ICONS = [
  "http",
  "graphql",
  "grpc",
  "websocket",
  "sse",
  "collection",
  "folder",
  "environment",
  "mcp",
  "import",
  "service",
  "diagram",
  "trace",
  "spark",
] as const;

export type NewIcon = (typeof NEW_ICONS)[number];

export const isNewIcon = (name: unknown): name is NewIcon => NEW_ICONS.includes(name as NewIcon);
