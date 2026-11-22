// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Loads the core (rt-wasm) on first use. The module is built by `scripts/build-wasm.mjs` into
// `public/wasm/` (gitignored) and imported at run time, so type checks, the mock backend and
// UI tests never need it. The types below mirror the rt-wasm JavaScript API (crate docs of
// `crates/rt-wasm/src/lib.rs`).

import {
  BackendError,
  type Environment,
  type FileChange,
  type GitDiff,
  type ImportFormat,
  type ImportPlan,
  type LineSlice,
  type Preview,
  type RequestSpec,
  type SearchHit,
  type SecretFinding,
  type SendReport,
  type StoredResponse,
  type TreeNode,
} from "@requesttailor/ui";

import type { CoreSendServices } from "./send-services";

/** `rt_app::SendInput`: what the UI sends plus the secret values the shell adds. */
export interface CoreSendInput {
  environment: string | null;
  vars: [string, string][];
  secrets: [string, string][];
}

export interface CompanionCredentials {
  url: string;
  token: string;
}

export interface PreparedSave {
  readonly changes: FileChange[];
  free(): void;
}

export interface CoreWorkspace {
  name(): string;
  tree(): TreeNode;
  request(path: string): RequestSpec;
  createRequest(folder: string, name: string): string;
  /** Wave 6: makes sure a folder exists (by directory or display name); returns its path. */
  createFolder(path: string): string;
  /** Wave 6: the context of a plugin tab and the URLs `{request}` covers (`TabScope`). */
  pluginScope(requestPath: string, environment?: string): unknown;
  updateRequest(path: string, spec: RequestSpec): void;
  dirty(): string[];
  prepareSave(): PreparedSave;
  commit(prepared: PreparedSave): void;
  environments(): Environment[];
  updateEnvironment(env: Environment): void;
  deleteEnvironment(name: string): void;
  preview(path: string, input: CoreSendInput): Preview;
  send(
    path: string,
    input: CoreSendInput,
    companion: CompanionCredentials | null,
    services: CoreSendServices | null,
  ): Promise<StoredResponse>;
  responseLines(id: number, start: number, count: number): LineSlice;
  responseBytes(id: number, offset: number, length: number): Uint8Array;
  filterJson(id: number, expression: string): string;
  releaseResponse(id: number): void;
  search(query: string, limit: number): SearchHit[];
  free(): void;
}

/** `Importer` (crates/rt-wasm/src/import_js.rs). */
export interface CoreImporter {
  analyze(files: { name: string; data: Uint8Array }[], format: ImportFormat | null): ImportPlan;
  /** The new collection's files; the plan is gone afterwards. */
  take(id: number): FileChange[];
  discard(id: number): void;
  free(): void;
}

export type CoreFiles = Map<string, Uint8Array | string>;

export interface CoreModule {
  default(options?: { module_or_path?: string | URL }): Promise<unknown>;
  Workspace: { open(files: CoreFiles): CoreWorkspace };
  send(files: CoreFiles, requestPath: string, options?: Record<string, unknown>): Promise<SendReport>;
  Importer: new () => CoreImporter;
  textDiff(path: string, old: Uint8Array | null, current: Uint8Array | null): GitDiff;
  /** `rt_app::secret_scan::scan` of `[path, text]` pairs (wave 5). */
  scanSecrets(files: [string, string][]): SecretFinding[];
  /** The message of a refused commit (`AppError::SecretsFound`); `findings` not empty. */
  secretRefusal(findings: SecretFinding[]): string;
}

// An absolute URL: Vite's dev server would otherwise mark the import as a module import and
// refuse to serve a file from `public/` for it.
const moduleUrl = () => new URL(`${import.meta.env.BASE_URL}wasm/rt_wasm.js`, location.href).href;

let loading: Promise<CoreModule> | null = null;

/** The initialized core. Rejects with a `BackendError` (kind `shell`) if it is not built. */
export function loadCore(): Promise<CoreModule> {
  loading ??= (async () => {
    let mod: CoreModule;
    try {
      mod = (await import(/* @vite-ignore */ moduleUrl())) as CoreModule;
    } catch {
      loading = null;
      throw new BackendError(
        "shell",
        "The RequestTailor core (WebAssembly) is not built. Run `node scripts/build-wasm.mjs` and reload.",
      );
    }
    await mod.default();
    return mod;
  })();
  return loading;
}

/** Turns an error thrown by the core (`AppError` with `kind`, or `TypeError`) into a
 *  `BackendError`. */
export function toBackendError(e: unknown): BackendError {
  if (e instanceof BackendError) return e;
  if (e instanceof Error) {
    const kind = (e as Error & { kind?: unknown }).kind;
    return new BackendError(typeof kind === "string" ? kind : "shell", e.message);
  }
  return new BackendError("shell", String(e));
}
