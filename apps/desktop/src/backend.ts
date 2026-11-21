// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The desktop `Backend`: every method is one Tauri command (src-tauri/src/commands.rs), which
// forwards to rt-app. Files, keychain and network stay on the Rust side; the webview only
// calls these commands (src-tauri/capabilities/default.json allows nothing else).

import {
  BackendError,
  type Backend,
  type CollectionInfo,
  type CompanionStatus,
  type Environment,
  type FileChange,
  type LineSlice,
  type Preview,
  type RequestSpec,
  type SecretsApi,
  type SendInput,
  type StoredResponse,
  type TransportApi,
  type TransportMode,
  type TreeNode,
} from "@requesttailor/ui";

/** All commands the webview may call; must match `capabilities/default.json`. */
export const COMMANDS = [
  "open_collection",
  "tree",
  "get_request",
  "create_request",
  "update_request",
  "dirty",
  "save",
  "environments",
  "update_environment",
  "delete_environment",
  "preview",
  "send",
  "response_lines",
  "response_bytes",
  "filter_json",
  "release_response",
  "secret_has",
  "secret_get",
  "secret_set",
  "secret_delete",
] as const;

export type Command = (typeof COMMANDS)[number];

/** `invoke` from `@tauri-apps/api/core`, injectable for tests. */
export type Invoke = (cmd: Command, args?: Record<string, unknown>) => Promise<unknown>;

/** Turns a rejected command (`CommandError`: the `AppError` kind and a readable message) into a
 *  `BackendError`. Anything else (a Tauri error string) becomes kind `shell`. */
export function toBackendError(e: unknown): BackendError {
  if (e instanceof BackendError) return e;
  if (e && typeof e === "object" && "kind" in e) {
    const { kind, message } = e as { kind: unknown; message?: unknown };
    return new BackendError(String(kind), message === undefined ? String(kind) : String(message));
  }
  return new BackendError("shell", e instanceof Error ? e.message : String(e));
}

const NO_COMPANION: CompanionStatus = {
  available: false,
  paired: false,
  url: null,
  howTo: "The desktop app sends requests itself; no companion is needed.",
};

export class TauriBackend implements Backend {
  readonly kind = "tauri" as const;
  readonly secrets: SecretsApi;
  readonly transport: TransportApi;

  constructor(private readonly invokeRaw: Invoke) {
    this.secrets = {
      has: (environment, variable) => this.call("secret_has", { environment, variable }),
      get: (environment, variable) => this.call("secret_get", { environment, variable }),
      set: (environment, variable, value) =>
        this.call("secret_set", { environment, variable, value }),
      delete: (environment, variable) => this.call("secret_delete", { environment, variable }),
    };
    this.transport = {
      mode: (): TransportMode => "native",
      companion: async () => ({ ...NO_COMPANION }),
      useCompanion: async (enabled) => {
        if (enabled) {
          throw new BackendError("unsupported", "The desktop app has no companion mode.");
        }
      },
    };
  }

  private async call<T>(cmd: Command, args?: Record<string, unknown>): Promise<T> {
    try {
      return (await this.invokeRaw(cmd, args)) as T;
    } catch (e) {
      throw toBackendError(e);
    }
  }

  openCollection(): Promise<CollectionInfo | null> {
    return this.call("open_collection");
  }
  tree(): Promise<TreeNode> {
    return this.call("tree");
  }
  getRequest(path: string): Promise<RequestSpec> {
    return this.call("get_request", { path });
  }
  createRequest(folder: string, name: string): Promise<string> {
    return this.call("create_request", { folder, name });
  }
  updateRequest(path: string, spec: RequestSpec): Promise<void> {
    return this.call("update_request", { path, spec });
  }
  dirty(): Promise<string[]> {
    return this.call("dirty");
  }
  save(): Promise<FileChange[]> {
    return this.call("save");
  }
  environments(): Promise<Environment[]> {
    return this.call("environments");
  }
  updateEnvironment(env: Environment): Promise<void> {
    return this.call("update_environment", { env });
  }
  deleteEnvironment(name: string): Promise<void> {
    return this.call("delete_environment", { name });
  }
  preview(path: string, input: SendInput): Promise<Preview> {
    return this.call("preview", { path, input: sendInput(input) });
  }
  send(path: string, input: SendInput): Promise<StoredResponse> {
    return this.call("send", { path, input: sendInput(input) });
  }
  responseLines(id: number, start: number, count: number): Promise<LineSlice> {
    return this.call("response_lines", { id, start, count });
  }
  async responseBytes(id: number, offset: number, length: number): Promise<Uint8Array> {
    // Binary IPC: the command answers with an ArrayBuffer, not a JSON array.
    const raw = await this.call<ArrayBuffer | Uint8Array | number[]>("response_bytes", {
      id,
      offset,
      length,
    });
    if (raw instanceof Uint8Array) return raw;
    if (raw instanceof ArrayBuffer) return new Uint8Array(raw);
    return Uint8Array.from(raw);
  }
  filterJson(id: number, expression: string): Promise<string> {
    return this.call("filter_json", { id, expression });
  }
  releaseResponse(id: number): Promise<void> {
    return this.call("release_response", { id });
  }
}

/** Only what the UI decides; secret values are added on the Rust side from the keychain. */
function sendInput(input: SendInput): SendInput {
  return { environment: input.environment, vars: input.vars };
}
