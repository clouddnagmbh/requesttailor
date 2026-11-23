// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The built-in tiles of the "New…" dialog (wave 14, strand B). Extensions add theirs with
// `AppExtension.newItems` (./items.ts); this list is the interface the dialog, the palette and
// `AppState.openNew({ kind })` share.

/** What the dialog can create itself. */
export type NewItemKind =
  | "http"
  | "graphql"
  | "grpc"
  | "websocket"
  | "sse"
  | "collection"
  | "folder"
  | "environment"
  | "mcp-test";

export interface NewItemInfo {
  kind: NewItemKind;
  title: string;
  description: string;
  group: "Requests" | "Organise" | "Testing";
  /** An icon of `./icons.ts`. */
  icon: string;
  /** Needs an open collection (all but "collection"). */
  needsCollection: boolean;
}

export const NEW_ITEMS: readonly NewItemInfo[] = [
  { kind: "http", title: "HTTP request", description: "REST and any other HTTP API.", group: "Requests", icon: "http", needsCollection: true },
  { kind: "graphql", title: "GraphQL", description: "Query and variables, sent as JSON.", group: "Requests", icon: "graphql", needsCollection: true },
  { kind: "grpc", title: "gRPC", description: "Unary and streaming calls from .proto or reflection.", group: "Requests", icon: "grpc", needsCollection: true },
  { kind: "websocket", title: "WebSocket", description: "Connect, send saved messages, watch frames.", group: "Requests", icon: "websocket", needsCollection: true },
  { kind: "sse", title: "Server-Sent Events", description: "Read a text/event-stream as it arrives.", group: "Requests", icon: "sse", needsCollection: true },
  { kind: "collection", title: "Collection", description: "A new folder of requests, with a first environment.", group: "Organise", icon: "collection", needsCollection: false },
  { kind: "folder", title: "Folder", description: "Group requests inside a collection.", group: "Organise", icon: "folder", needsCollection: true },
  { kind: "environment", title: "Environment", description: "Variables for one stage, secrets in the keychain.", group: "Organise", icon: "environment", needsCollection: true },
  { kind: "mcp-test", title: "MCP server test", description: "Call a tool of an MCP server and keep it as a test.", group: "Testing", icon: "mcp", needsCollection: true },
];

export const isNewItemKind = (k: unknown): k is NewItemKind => NEW_ITEMS.some((i) => i.kind === k);
