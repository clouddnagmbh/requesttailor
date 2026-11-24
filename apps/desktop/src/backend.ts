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
  type CompareApi,
  type CookieApi,
  type RescueApi,
  type RunnerApi,
  type StreamsApi,
  type ScriptsApi,
  type Environment,
  type FileChange,
  type GitApi,
  type HistoryApi,
  type ImportApi,
  type OpenApiApi,
  type LineSlice,
  type PluginApi,
  type Preview,
  type RecentCollection,
  type RequestSpec,
  type SearchHit,
  type AiApi,
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
import { tauriCompanions } from "./companions";
import { tauriFolderScripts } from "./folder-scripts";
import { tauriVariables } from "./variables";
import { tauriAi, type MakeAiChannel } from "./ai";
// Wave 12: one file per strand.
import { tauriCompare, type MakeStepChannel } from "./compare";
import { tauriContract } from "./contract";
import { tauriOpenApi } from "./openapi";
import { tauriDiffs } from "./diffs";
import { tauriMerges } from "./merges";
import { tauriRescue, type MakeProgressChannel } from "./rescue";
import { tauriReview } from "./review";
import { tauriTimeline } from "./timeline";
import { tauriScripts } from "./scripts";
import { tauriGit } from "./git";
import { tauriCollections } from "./collections";
import { tauriRepositories } from "./repositories";
import { tauriShare } from "./share";
import { tauriSnapshots } from "./snapshots";
import { tauriEnvCompare } from "./envcompare";
import { tauriEgress } from "./egress";
import { tauriJournal } from "./journal";
import { tauriAccess } from "./access";
import { tauriShrink } from "./shrink";
import { tauriConsumers } from "./consumers";
import { tauriSequence } from "./sequence";
import { tauriSecurity } from "./security";
import { tauriNotebook } from "./notebook";
import { tauriDebugger } from "./debugger";

import { tauriInspect } from "./inspect";
import { tauriPaging } from "./paging";
import { tauriInbox } from "./inbox";
import { tauriLint } from "./lint";
import { tauriViews } from "./views";
import { tauriCatalogue } from "./catalogue";
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
  "collections_current",
  "collections_add",
  "collections_remove_root",
  "collections_activate",
  "collections_tree",
  "collections_dirty",
  "collections_save_all",
  "collections_search",
  "collections_create",
  "collections_reveal",
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
  "git_pick_repository",
  "git_identity",
  "git_set_identity",
  "git_has_credentials",
  "git_set_credentials",
  "git_delete_credentials",
  "import_analyze",
  "import_write",
  "import_discard",
  "import_adopt",
  "import_write_into",
  "companion_list",
  "companion_read",
  "companion_stage",
  "variables_scope",
  "variables_define",
  "folder_scripts_get",
  "folder_scripts_set",
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
  "ai_settings",
  "ai_set_settings",
  "ai_has_profile_key",
  "ai_set_profile_key",
  "ai_delete_profile_key",
  "ai_policy",
  "ai_set_device_opt_out",
  "ai_preview",
  "ai_confirm_preview",
  "ai_complete",
  "ai_cancel",
  "agents_policy",
  "agents_set_policy",
  "agents_pending",
  "agents_decide",
  "agents_audit",
  "agents_check",
  "agents_check_spec",
  // Wave 8, strand B.
  "grpc_services",
  "grpc_template",
  "graphql_schema",
  // Wave 8, strand A.
  "stream_open",
  "stream_send",
  "stream_finish",
  "stream_close",
  "stream_log",
  "stream_release",
  "mcp_connect",
  "mcp_tools",
  "mcp_call",
  "mcp_disconnect",
  "mcp_tests",
  "mcp_get_test",
  "mcp_save_test",
  "mcp_run_test",
  "mcp_resources",
  "mcp_read_resource",
  "mcp_prompts",
  "mcp_get_prompt",
  "mcp_self_server",
  "mcp_demo_status",
  "mcp_demo_start",
  "mcp_demo_stop",
  // Wave 12 (interface): revisions.
  "git_rev_parse",
  "git_tree_at",
  "git_show",
  "git_file_log",
  "git_merge_base",
  "git_commits",
  // Wave 12, strand A.
  "diff_working",
  "diff_commit",
  "diff_between",
  // Wave 12, strand B.
  "merge_conflicts",
  "merge_resolve",
  // Wave 12, strand C.
  "compare_run",
  "compare_revisions",
  "compare_bisect",
  "compare_cancel",
  // Wave 12, strand D.
  "timeline_history",
  "timeline_blame",
  "timeline_version",
  "timeline_results",
  // Wave 12, strand E.
  "review_hosts",
  "review_set_hosts",
  "review_repo",
  "review_has_token",
  "review_set_token",
  "review_delete_token",
  "review_start_login",
  "review_finish_login",
  "review_list",
  "review_open",
  "review_comments",
  "review_comment",
  "review_submit",
  "review_ci",
  // Wave 12, strand F.
  "contract_spec",
  "contract_check",
  "contract_changes",
  "contract_changelog",
  // Wave 16, strand D.
  "contract_changelog_facts",
  // OpenAPI wizard (src-tauri/src/openapi.rs).
  "openapi_fetch",
  "openapi_inspect",
  "openapi_plan",
  "openapi_requirements",
  "openapi_validate_response",
  // Wave 12, strand G.
  "rescue_scan",
  "rescue_rewrite",
  // Wave 14 (interface): Git graph (strand F) and wizards (strand G).
  "git_graph",
  "git_refs",
  "git_changes",
  "git_file_diff",
  "git_remotes",
  "git_stashes",
  "git_plan",
  "git_apply",
  "git_last_action",
  "git_undo",
  // Wave 15 (interface): Git client (strands A–F).
  "repositories_open",
  "repositories_activate",
  "repositories_summary",
  "repositories_clone",
  "repositories_init",
  "repositories_open_path",
  "git_working_copy",
  "git_file_tree",
  "git_blob",
  "git_blame",
  "git_search_history",
  "git_reflog",
  "git_commit_info",
  "git_pick",
  "git_write_tree",
  "git_write_commit",
  "git_resolve_text",
  "git_index_diff",
  "git_stage",
  "git_unstage",
  "git_stage_patch",
  "git_unstage_patch",
  "git_discard_patch",
  "git_commit_staged",
  "git_verify",
  "git_config",
  "review_create",
  // Wave 17.
  "snapshots_read",
  "snapshots_freeze",
  "snapshots_set_ignore",
  "snapshots_check",
  "snapshots_list",
  "share_link",
  "share_open",
  "share_copy_as",
  "share_qr",
  "contract_stage_spec",
  // Wave 19.
  "egress_hosts",
  "egress_allow",
  "egress_remove",
  "egress_request",
  "journal_settings",
  "journal_set_settings",
  "journal_entries",
  "journal_clear",
  "journal_plan_manual",
  "journal_plan_compensations",
  "journal_execute",
  "journal_plugin_rules",
  "journal_pre_read",
  "journal_states",
  "access_check",
  "shrink_run",
  "shrink_cancel",
  "shrink_predicate",
  "consumers_list",
  "consumers_verify",
  "sequence_model",
  "sequence_render",
  "sequence_clear",
  "security_check",
  "security_rules",
  "security_settings",
  "security_set_settings",
  "notebook_list",
  "notebook_read",
  "notebook_stage",
  "notebook_run",
  "debugger_start",
  "debugger_command",
  "debugger_state",
  "debugger_stop",
  // Wave 18.
  "envcompare_run",
  "inspect_jwt",
  "inspect_tokens",
  "inspect_checks",
  "inspect_ignore_check",
  "paging_rule",
  "paging_set_rule",
  "paging_run",
  "inbox_settings",
  "inbox_set_settings",
  "inbox_start",
  "inbox_stop",
  "inbox_status",
  "inbox_events",
  "inbox_get",
  "inbox_clear",
  "inbox_make_request",
  "lint_run",
  "lint_settings",
  "lint_set_settings",
  "lint_ignore",
  "views_list",
  "views_save",
  "views_remove",
  "views_apply",
  "views_languages",
  "catalogue_sources",
  "catalogue_add_source",
  "catalogue_remove_source",
  "catalogue_refresh",
  "catalogue_plan",
  "catalogue_install",
  "catalogue_installed",
  "catalogue_check_updates",
  "contract_schema_changes",
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
  /** Wave 14: the collections in the sidebar (`collections.ts`). */
  readonly collections = tauriCollections((cmd, args) => this.call(cmd, args));
  /** Wave 15: repositories in the set (`repositories.ts`). */
  readonly repositories = tauriRepositories((cmd, args) => this.call(cmd, args));
  /** Wave 19 (one file per group, `<name>.ts`). */
  readonly egress = tauriEgress((cmd, args) => this.call(cmd, args));
  readonly journal = tauriJournal((cmd, args) => this.call(cmd, args));
  readonly access = tauriAccess((cmd, args) => this.call(cmd, args));
  readonly shrink = tauriShrink((cmd, args) => this.call(cmd, args));
  readonly consumers = tauriConsumers((cmd, args) => this.call(cmd, args));
  readonly sequence = tauriSequence((cmd, args) => this.call(cmd, args));
  readonly security = tauriSecurity((cmd, args) => this.call(cmd, args));
  readonly notebook = tauriNotebook((cmd, args) => this.call(cmd, args));
  readonly debugger = tauriDebugger((cmd, args) => this.call(cmd, args));
  /** Wave 18 (one file per group, `<name>.ts`). */
  readonly envCompare = tauriEnvCompare((cmd, args) => this.call(cmd, args));
  readonly inspect = tauriInspect((cmd, args) => this.call(cmd, args));
  readonly paging = tauriPaging((cmd, args) => this.call(cmd, args));
  readonly inbox = tauriInbox((cmd, args) => this.call(cmd, args));
  readonly lint = tauriLint((cmd, args) => this.call(cmd, args));
  readonly views = tauriViews((cmd, args) => this.call(cmd, args));
  readonly catalogue = tauriCatalogue((cmd, args) => this.call(cmd, args));
  /** Wave 17: snapshot tests (`snapshots.ts`) and sharing (`share.ts`). */
  readonly snapshots = tauriSnapshots((cmd, args) => this.call(cmd, args));
  readonly share = tauriShare((cmd, args) => this.call(cmd, args));
  readonly secrets: SecretsApi;
  readonly transport: TransportApi;
  readonly git: GitApi;
  readonly imports: ImportApi;
  readonly openapi: OpenApiApi = tauriOpenApi((cmd, args) => this.call(cmd, args));
  readonly history: HistoryApi;
  readonly plugins: PluginApi;
  readonly auth: AuthApi;
  readonly cookies: CookieApi;
  readonly scripts: ScriptsApi;
  readonly runner: RunnerApi;
  /** Wave 8 (one file per strand). */
  readonly streams: StreamsApi;
  readonly grpc = tauriGrpc((cmd, args) => this.call(cmd, args));
  readonly graphql = tauriGraphQl((cmd, args) => this.call(cmd, args));
  readonly agents = tauriAgents((cmd, args) => this.call(cmd, args));
  readonly mcp = tauriMcp((cmd, args) => this.call(cmd, args));
  readonly ai: AiApi;
  /** Wave 12 (one file per strand). */
  readonly diffs = tauriDiffs((cmd, args) => this.call(cmd, args));
  readonly merges = tauriMerges((cmd, args) => this.call(cmd, args));
  readonly compare: CompareApi;
  readonly timeline = tauriTimeline((cmd, args) => this.call(cmd, args));
  readonly review = tauriReview((cmd, args) => this.call(cmd, args));
  readonly contract = tauriContract((cmd, args) => this.call(cmd, args));
  /** Wave 13. */
  readonly companions = tauriCompanions((cmd, args) => this.call(cmd, args));
  /** Wave 14: variable completion. */
  readonly variables = tauriVariables((cmd, args) => this.call(cmd, args), sendInput);
  /** Wave 16: scripts of the collection and its folders (suggested edits). */
  readonly folderScripts = tauriFolderScripts((cmd, args) => this.call(cmd, args));
  readonly rescue: RescueApi;

  constructor(
    private readonly invokeRaw: Invoke,
    makeChannel?: MakeChannel,
    /** Wave 8: the channel of `streams.open` (injectable for tests, like `makeChannel`). */
    makeStreamChannel?: MakeStreamChannel,
    /** Wave 12: the channel of `compare.bisect` steps (injectable for tests, like `makeChannel`). */
    makeStepChannel?: MakeStepChannel,
    /** Wave 12: the channel of `rescue.scanHistory` progress (injectable for tests). */
    makeProgressChannel?: MakeProgressChannel,
    /** Wave 16: the channel of `ai.complete` events (injectable for tests). */
    makeAiChannel?: MakeAiChannel,
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
    this.compare = tauriCompare(call, makeStepChannel);
    this.rescue = tauriRescue(call, makeProgressChannel);
    this.ai = tauriAi(call, makeAiChannel);
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
