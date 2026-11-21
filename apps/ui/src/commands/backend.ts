// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The command layer: the only way the UI reaches the core. Components never import rt-wasm,
// Tauri APIs or files directly; they call a `Backend`. There are three implementations:
//   - Tauri (apps/desktop): Rust commands over `invoke`, backed by rt-app.
//   - Web (apps/web): rt-wasm, backed by the same rt-app, plus browser file and secret storage.
//   - Mock (./mock.ts): in memory, for UI development and Playwright tests of the UI strands.
// Adding or changing a command is an interface change (working rule 1): no new commands
// without agreement; list missing ones in your report.

import type {
  Environment,
  FileChange,
  LineSlice,
  RequestSpec,
  StoredResponse,
  TreeNode,
} from "./types";

export interface CollectionInfo {
  /** Stable id for this collection on this device (used as `SecretKey.collection`). */
  id: string;
  /** Display name (collection name or folder name). */
  name: string;
  /** Where it lives, for display only (path on desktop, directory name in the browser). */
  location: string;
}

export interface SendInput {
  environment: string | null;
  /** Runtime variables, highest precedence. */
  vars: [string, string][];
}

/** Where requests go out from. */
export type TransportMode = "native" | "browser" | "companion";

export interface CompanionStatus {
  /** A companion (`rt agent`) answers on the local machine. */
  available: boolean;
  /** This browser is paired with it and may use it. */
  paired: boolean;
  /** Base URL of the companion, if available. */
  url: string | null;
  /** One-line instruction for the user when it is not available, e.g. "Run `rt agent`". */
  howTo: string;
}

/** Secret values of the open collection. Values never go into collection files. */
export interface SecretsApi {
  has(environment: string, variable: string): Promise<boolean>;
  /** For "reveal" in the UI. */
  get(environment: string, variable: string): Promise<string | null>;
  set(environment: string, variable: string, value: string): Promise<void>;
  delete(environment: string, variable: string): Promise<void>;
}

export interface TransportApi {
  /** Current mode. Desktop: always "native". Web: "browser" or "companion". */
  mode(): TransportMode;
  /** Web only; desktop reports `available: false`. */
  companion(): Promise<CompanionStatus>;
  /** Web only: switch between direct browser fetch and the companion. */
  useCompanion(enabled: boolean): Promise<void>;
}

export interface Backend {
  readonly kind: "tauri" | "web" | "mock";

  /** Shows the platform's folder picker and opens the collection; `null` if cancelled. */
  openCollection(): Promise<CollectionInfo | null>;
  tree(): Promise<TreeNode>;
  getRequest(path: string): Promise<RequestSpec>;
  /** New GET request named `name` in `folder` (`""` = root). Returns its path. Unsaved. */
  createRequest(folder: string, name: string): Promise<string>;
  /** Replaces a request in memory. Unsaved until `save`. */
  updateRequest(path: string, spec: RequestSpec): Promise<void>;
  /** Request paths with unsaved changes. */
  dirty(): Promise<string[]>;
  /** Writes all unsaved changes to the collection's files and returns what was written. */
  save(): Promise<FileChange[]>;
  /** Environments as in the files; secret variables have `value: null`. */
  environments(): Promise<Environment[]>;

  /** Sends the saved-or-unsaved current state of the request. Secrets are added by the shell. */
  send(path: string, input: SendInput): Promise<StoredResponse>;
  responseLines(id: number, start: number, count: number): Promise<LineSlice>;
  responseBytes(id: number, offset: number, length: number): Promise<Uint8Array>;
  /** JSONPath over a JSON body; returns pretty JSON. Rejects with a message on error. */
  filterJson(id: number, expression: string): Promise<string>;
  releaseResponse(id: number): Promise<void>;

  readonly secrets: SecretsApi;
  readonly transport: TransportApi;
}

/** Errors from a backend are rejected promises carrying this shape. */
export class BackendError extends Error {
  constructor(
    readonly kind: string,
    message: string,
  ) {
    super(message);
    this.name = "BackendError";
  }
}
