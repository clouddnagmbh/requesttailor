// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The desktop `Backend`: every method is one Tauri command (src-tauri/src/commands.rs), which
// forwards to rt-app. Files, keychain and network stay on the Rust side; the webview only
// calls these commands (src-tauri/capabilities/default.json allows nothing else).

import {
  BackendError,
  type AuthApi,
  type Backend,
  type CollectionInfo,
  type CompanionStatus,
  type CookieApi,
  type RunnerApi,
  type StreamsApi,
  type ScriptsApi,
  type Environment,
  type FileChange,
  type GitApi,
  type HistoryApi,
  type ImportApi,
  type LineSlice,
  type PluginApi,
  type Preview,
  type RecentCollection,
  type RequestSpec,
  type SearchHit,
  type SecretsApi,
  type SendInput,
  type StoredResponse,
  type TransportApi,
  type TransportMode,
  type TreeNode,
} from "@requesttailor/ui";

import { tauriAuth } from "./auth";
import { tauriCookies } from "./cookies";
import { tauriRunner, type MakeChannel } from "./runner";
import { tauriStreams, type MakeStreamChannel } from "./streams";
import { tauriGrpc } from "./grpc";
import { tauriGraphQl } from "./graphql";
import { tauriAgents } from "./agents";
import { tauriMcp } from "./mcp-client";
import { tauriAi } from "./ai";
import { tauriScripts } from "./scripts";
import { tauriGit } from "./git";
import { tauriHistory } from "./history";
import { tauriImports } from "./import";
import { tauriPlugins } from "./plugins";

/** All commands the webview may call; must match `capabilities/default.json`. */
export const COMMANDS = [
  "open_collection",
  "tree",
  "get_request",
  "create_request",
  "create_folder",
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
  "git_status",
  "git_diff",
  "git_commit",
  "git_log",
  "git_branches",
  "git_checkout",
  "git_create_branch",
  "git_push",
  "git_pull",
  "git_conflict",
  "git_resolve",
  "git_init",
  "git_clone",
  "git_identity",
  "git_set_identity",
  "git_has_credentials",
  "git_set_credentials",
  "git_delete_credentials",
  "import_analyze",
  "import_write",
  "import_discard",
  "history_list",
  "history_clear",
  "search",
  "recent_collections",
  "open_recent",
  "git_scan",
  "import_analyze_plugin",
  "plugin_list",
  "plugin_inspect",
  "plugin_install",
  "plugin_remove",
  "plugin_set_enabled",
  "plugin_render_tab",
  "plugin_action",
  "plugin_welcome",
  "plugin_choose_welcome",
  "auth_tokens",
  "auth_fetch_token",
  "auth_clear_tokens",
  "auth_cancel_prompt",
  "cookie_list",
  "cookie_delete",
  "cookie_clear",
  "scripts_trust",
  "scripts_set_trusted",
  "scripts_session_vars",
  "scripts_set_session_var",
  "scripts_clear_session_vars",
  "runner_plan",
  "runner_parse_data",
  "runner_run",
  "runner_cancel",
  "ai_config",
  "ai_set_config",
  "ai_has_key",
  "ai_set_key",
  "ai_delete_key",
  "ai_ask",
  "agents_policy",
  "agents_set_policy",
  "agents_pending",
  "agents_decide",
  "agents_audit",
  // Wave 8, strand A.
  "stream_open",
  "stream_send",
  "stream_finish",
  "stream_close",
  "stream_log",
  "stream_release",
] as const;

export type Command = (typeof COMMANDS)[number];

/** `invoke` from `@tauri-apps/api/core`, injectable for tests. */
export type Invoke = (cmd: Command, args?: Record<string, unknown>) => Promise<unknown>;

/** A command call that rejects with `BackendError`; what the per-feature files get. */
export type Call = <T>(cmd: Command, args?: Record<string, unknown>) => Promise<T>;

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
  readonly git: GitApi;
  readonly imports: ImportApi;
  readonly history: HistoryApi;
  readonly plugins: PluginApi;
  readonly auth: AuthApi;
  readonly cookies: CookieApi;
  readonly scripts: ScriptsApi;
  readonly runner: RunnerApi;
  /** Wave 8 (one file per strand). */
  readonly streams: StreamsApi;
  readonly grpc = tauriGrpc();
  readonly graphql = tauriGraphQl();
  readonly agents = tauriAgents((cmd, args) => this.call(cmd, args));
  readonly mcp = tauriMcp();
  readonly ai = tauriAi((cmd, args) => this.call(cmd, args));

  constructor(
    private readonly invokeRaw: Invoke,
    makeChannel?: MakeChannel,
    /** Wave 8: the channel of `streams.open` (injectable for tests, like `makeChannel`). */
    makeStreamChannel?: MakeStreamChannel,
  ) {
    const call: Call = (cmd, args) => this.call(cmd, args);
    this.git = tauriGit(call);
    this.imports = tauriImports(call);
    this.history = tauriHistory(call);
    this.plugins = tauriPlugins(call);
    this.auth = tauriAuth(call);
    this.cookies = tauriCookies(call);
    this.scripts = tauriScripts(call);
    this.runner = tauriRunner(call, makeChannel);
    this.streams = tauriStreams(call, makeStreamChannel);
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
  recentCollections(): Promise<RecentCollection[]> {
    return this.call("recent_collections");
  }
  openRecent(id: string, interactive: boolean): Promise<CollectionInfo | null> {
    return this.call("open_recent", { id, interactive });
  }
  search(query: string, limit: number): Promise<SearchHit[]> {
    return this.call("search", { query, limit });
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
  createFolder(path: string): Promise<string> {
    return this.call("create_folder", { path });
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
