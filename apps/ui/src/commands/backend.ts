// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The command layer: the only way the UI reaches the core. Components never import rt-wasm,
// Tauri APIs or files directly; they call a `Backend`. There are three implementations:
//   - Tauri (apps/desktop): Rust commands over `invoke`, backed by rt-app.
//   - Web (apps/web): rt-wasm, backed by the same rt-app, plus browser file and secret storage.
//   - Mock (./mock.ts): in memory, for UI development and Playwright tests of the UI strands.
// Adding or changing a command is an interface change (working rule 1): no new commands
// without agreement; list missing ones in your report.
//
// Wave 14: the sidebar shows several collections (`collections`, `CollectionSetApi`). "The open
// collection" in the comments below means the *active* collection of that set: every
// per-collection command acts on the collection that is active when it is called, and
// long-running calls (runner, streams, compare, bisect, MCP tests) keep it until they end.

import { BackendError, type GitApi, type GitRepositoryApi, type ReviewApi } from "@tailor-kit/git-ui/api";

import type {
  CopiedAs,
  CopyKind,
  CopyOptions,
  FreezeOptions,
  ShareLink,
  SharePayload,
  ShareOptions,
  Snapshot,
  SnapshotCheck,
  SnapshotEntry,
  SnapshotFreeze,
  Environment,
  Script,
  FileChange,
  HistoryEntry,
  ImportFormat,
  AuthPrompt,
  CookieInfo,
  ImportPlan,
  FetchedSpec,
  OpenApiOptions,
  ResponseCheck,
  SendRequirements,
  SpecOverview,
  ExternalImport,
  FolderImport,
  LineSlice,
  Manifest,
  PluginInfo,
  Preview,
  WelcomeState,
  RequestSpec,
  SearchHit,
  StoredResponse,
  TabTarget,
  TokenInfo,
  TreeNode,
  ViewReply,
  DataRow,
  RunEvent,
  RunPlan,
  RunSummary,
  ScriptTrust,
  VarChange,
  AgentPolicy,
  AiAnswer,
  AiCollectionPolicy,
  AiConfig,
  AiEvent,
  AiPreview,
  AiProviderKind,
  AiReply,
  AiRequest,
  AiSettings,
  AiTask,
  ChangelogFacts,
  ApprovalDecision,
  AgentCheck,
  AuditEntry,
  GrpcService,
  McpCallResult,
  McpPrompt,
  McpPromptResult,
  McpResource,
  McpResourceContents,
  McpSelfServer,
  McpServerConfig,
  McpServerInfo,
  McpTest,
  McpTestResult,
  McpTool,
  MessageVariant,
  PendingApproval,
  StreamEvent,
  StreamLog,
  BisectOutcome,
  BisectRequest,
  BisectStep,
  CollectionDiff,
  CollectionChanges,
  CollectionSearchHit,
  CollectionSet,
  NewCollection,
  CompareRequest,
  ConflictChoice,
  ContractReport,
  ContractSpec,
  FieldBlame,
  HistoryScan,
  MergeState,
  RequestTimeline,
  RescuePlan,
  RescueResult,
  RevisionRun,
  RevisionRunRequest,
  RunComparison,
  ScanProgress,
  SpecChanges,
  TestRecord,
  Variable,
  VariableScope,
} from "./types";
import type {
  CapturedRequest,
  CatalogueSource,
  CatalogueUpdate,
  CheckScope,
  EffectivePaging,
  EnvCompareRequest,
  EnvComparison,
  FoundToken,
  InboxDraft,
  InboxPage,
  InboxSettings,
  InboxStatus,
  InspectTarget,
  InstallPlan,
  InstalledSource,
  JwtInfo,
  LintReport,
  LintRule,
  LintSettings,
  Observation,
  PagedResult,
  PagingRuleScope,
  PagingRun,
  PagingSpec,
  ResponseView,
  SchemaKind,
  SourceIndex,
  ViewLanguage,
  ViewResult,
} from "./types";

import type {
  AccessCheck,
  AccessOptions,
  AccessProfile,
  CompensationItem,
  ConsumerRef,
  ConsumerReport,
  ConsumerVerifyRequest,
  DebugCommand,
  DebugState,
  DebugTarget,
  EgressHost,
  EgressRequest,
  EgressResponse,
  EntryStates,
  JournalPage,
  JournalQuery,
  JournalSettings,
  ManualRevert,
  Notebook,
  NotebookEntry,
  NotebookOutcome,
  NotebookRun,
  PluginCompensationRule,
  PreRead,
  RevertOptions,
  RevertOutcome,
  RevertPlan,
  SecurityFinding,
  SecurityRule,
  SecuritySettings,
  SecurityTarget,
  SequenceModel,
  SequenceOptions,
  SequenceSource,
  SequenceText,
  ShrinkPredicate,
  ShrinkRequest,
  ShrinkResult,
} from "./types";

// The Git, repository and review calls the Git views make live in `packages/git-ui` (step K3).
export { BackendError };
export type { GitApi, ReviewApi };

export interface CollectionInfo {
  /** Stable id for this collection on this device (used as `SecretKey.collection`). */
  id: string;
  /** Display name (collection name or folder name). */
  name: string;
  /** Where it lives, for display only (path on desktop, directory name in the browser). */
  location: string;
}

/** A collection opened before on this device (most recent first). */
export interface RecentCollection extends CollectionInfo {
  /** Unix milliseconds. */
  openedAt: number;
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

/** Wave 15 (interface, strand A): repositories in the collection set. The list is part of
 *  `CollectionSet` (`repositories`, `active_repository`); these calls change it. The calls the
 *  Git views need are `GitRepositoryApi` (`@tailor-kit/git-ui`). */
export interface RepositoryApi extends GitRepositoryApi {
  /** Folder picker; adds the folder as a root: its collections, or, without any, the Git
   *  repository it is in as a repository root (outside Git: a new collection, as `add`).
   *  `null` if cancelled. */
  open(): Promise<CollectionSet | null>;
}

/** Semantic diffs: per request, folder, collection and environment field, secrets only by
 *  name. Owner in wave 12: strand A. */
export interface DiffApi {
  /** The saved files against `HEAD`. */
  working(): Promise<CollectionDiff>;
  /** A commit against its first parent. */
  commit(rev: string): Promise<CollectionDiff>;
  /** `from` → `to` (branch compare, PR review: merge base → head). */
  between(from: string, to: string): Promise<CollectionDiff>;
}

/** Semantic merge: `git.pull` merges on the model; what is left are conflicts per field.
 *  Owner in wave 12: strand B. */
export interface MergeApi {
  /** The conflicts of the merge in progress (recomputed from Git's index). */
  conflicts(): Promise<MergeState>;
  /** Writes the merged files with these choices and marks them resolved (like
   *  `git.resolve`); refuses with unsaved changes; the next `git.commit` concludes the merge.
   *  The UI reloads afterwards. */
  resolve(choices: ConflictChoice[]): Promise<void>;
}

/** The collection at a revision, run without a checkout, with today's environment and
 *  secrets; comparisons and bisect. Owner in wave 12: strand C. */
export interface CompareApi {
  run(request: RevisionRunRequest, input: SendInput): Promise<RevisionRun>;
  compare(request: CompareRequest, input: SendInput): Promise<RunComparison>;
  /** The first commit between good and bad at which something fails; `onStep` after each
   *  tested commit. */
  bisect(request: BisectRequest, input: SendInput, onStep: (step: BisectStep) => void): Promise<BisectOutcome>;
  /** Stops the running compare or bisect. */
  cancel(): Promise<void>;
}

/** Time travel per request: history across renames, blame per field, old versions, test
 *  results per commit (recorded by the runner, kept in the data folder or IndexedDB, never
 *  in the repository). Sending or restoring an old version goes through `updateRequest`.
 *  Owner in wave 12: strand D. */
export interface TimelineApi {
  history(path: string, limit: number): Promise<RequestTimeline>;
  blame(path: string): Promise<FieldBlame[]>;
  /** The request as it was at `rev` (`path` as at that commit). */
  version(rev: string, path: string): Promise<RequestSpec>;
  /** Results recorded under the commit `rev` for this collection. */
  results(rev: string): Promise<TestRecord[]>;
}

/** The OpenAPI spec in the repository against the collection, and its changes over the
 *  history. Owner in wave 12: strand F. */
export interface ContractApi {
  /** The spec the collection is checked against; `null` if none is configured or found. */
  spec(): Promise<ContractSpec | null>;
  /** The collection against the spec, at `rev` (`null`: the saved files). */
  check(rev: string | null): Promise<ContractReport>;
  /** Changes of the spec from `from` to `to`, breaking ones marked. */
  changes(from: string, to: string): Promise<SpecChanges>;
  /** API changelog (Markdown) of the range, as `rt changelog`. */
  changelog(from: string, to: string): Promise<string>;
  /** Wave 16: the range's changelog and its parts as data, with the commits (first-parent
   *  chain), for writers of release notes. */
  changelogFacts(from: string, to: string): Promise<ChangelogFacts>;
  /** Wave 17: writes the spec the collection is checked against (`spec()`, also one at the
   *  repository root) with `text`, unsaved (`save` writes it, a commit scans it). Refuses any
   *  other file and a text that is no valid spec; returns the spec's path. */
  stageSpec(text: string): Promise<string>;
  /** Wave 18 (strand H): changes of a GraphQL schema (SDL) or Protobuf files from `from` to `to`,
   *  breaking ones marked, as `changes` does for the OpenAPI spec. The schema files are the
   *  ones `extensions.requesttailor.contract` names. */
  schemaChanges(kind: SchemaKind, from: string, to: string): Promise<SpecChanges>;
}

/** Credentials anywhere in the history, and rewriting it so they become variables. The
 *  value never reaches the UI; `rewrite` puts it into the secret store itself. Owner in
 *  wave 12: strand G. */
export interface RescueApi {
  scanHistory(onProgress?: (progress: ScanProgress) => void): Promise<HistoryScan>;
  /** Rewrites the local branches after the user confirmed (backup refs first); refuses with
   *  unsaved or uncommitted changes. The UI reloads afterwards. */
  rewrite(plan: RescuePlan): Promise<RescueResult>;
}

/** One file handed to `ImportApi.analyze`: its name as picked (only the last path component
 *  counts) and its bytes. Pasted text is a file named `pasted.txt`. */
export interface ImportSource {
  name: string;
  data: Uint8Array;
}

/** Import another tool's file as a new collection: `analyze` converts and reports what would
 *  be lost, `write` puts it into a new folder (`plan.folder_name` inside a folder the user
 *  picks) and opens it. Owner in wave 4: strand B. */
export interface ImportApi {
  /** Converts `files` as one collection: one export, or a collection with its environment
   *  files (Postman environments and globals, `http-client.env.json`), or several `.http` /
   *  cURL files (one folder each). `format: null` detects it. Rejects with kind `import` for
   *  unknown, invalid or mismatched files. */
  analyze(files: ImportSource[], format: ImportFormat | null): Promise<ImportPlan>;
  /** Converts with an installed plugin's importer (`ImporterContribution`); the plan's
   *  `format` is `plugin`. Owner in wave 5: strand A. */
  analyzeWithPlugin(files: ImportSource[], plugin: string, importer: string): Promise<ImportPlan>;
  /** Asks for a folder, writes the plan there as a new collection and adds it to the
   *  collection set as another root, active (wave 14; the only root if the set is empty).
   *  `null` if the user cancelled the folder picker (the plan stays available). */
  write(id: number): Promise<CollectionInfo | null>;
  discard(id: number): Promise<void>;
  /** Wave 13: adds plan `id` to the open collection as a new top-level folder named `name`
   *  (default: the plan's folder name; ` 2`, ` 3`, ... if taken), unsaved like any edit:
   *  structure, scripts, environments (only missing variables), other files below the folder.
   *  The plan is gone afterwards. */
  writeInto(id: number, name?: string | null): Promise<FolderImport>;
  /** Wave 13: checks and keeps a collection converted by an extension, so it is written like
   *  any import (`write`, `writeInto`, `discard`). Rejects with kind `import`. */
  adopt(imported: ExternalImport): Promise<ImportPlan>;
}

/** The OpenAPI import wizard and the run wizard ("Ready to send?"). Free. `fetch`, `inspect`
 *  and `plan` work without an open collection; `plan` returns an `ImportPlan` written with
 *  `ImportApi.write` / `writeInto` / `discard` like any import. `requirements` and
 *  `validateResponse` use the open collection's spec (`ContractApi.spec`), parsed once and
 *  cached per collection; both resolve `null` when there is no spec or no operation matches. */
export interface OpenApiApi {
  /** Fetches a spec and the relative files its `$ref`s name, over the core transport (web:
   *  the browser's fetch, then the companion if the server does not allow CORS). Rejects with
   *  kind `import` (HTTP status, transport error; web: a CORS hint). */
  fetch(url: string): Promise<FetchedSpec>;
  /** Reads a spec (one file, a multi-file spec, or a `.zip`) for the wizard. Rejects with
   *  kind `import`. */
  inspect(files: ImportSource[]): Promise<SpecOverview>;
  /** Converts with the wizard's choices. */
  plan(files: ImportSource[], options: OpenApiOptions): Promise<ImportPlan>;
  /** What the request at `path` still needs before sending (`input` as for `send`). */
  requirements(path: string, input: SendInput): Promise<SendRequirements | null>;
  /** The stored response `responseId` of the request at `path` against the spec. */
  validateResponse(path: string, responseId: number): Promise<ResponseCheck | null>;
}

/** Sends from this device. Recorded by the backend on every `send`; no bodies, no header
 *  values, URL with secrets masked. Owner in wave 4: strand E. */
export interface HistoryApi {
  /** Entries of the open collection matching `query` (empty: all), newest first. */
  list(query: string, limit: number): Promise<HistoryEntry[]>;
  /** Forgets the open collection's entries. */
  clear(): Promise<void>;
}

/** Installed plugins (rt-plugin, `crates/rt-plugin/API.md`). Desktop and CLI share one plugin
 *  directory; the web keeps plugins in browser storage. Owner in wave 5: strand A (install,
 *  permissions, enable, importers); tabs render `ViewReply.view` with the UI's plugin view
 *  (strand B). */
export interface PluginApi {
  list(): Promise<PluginInfo[]>;
  /** Reads the manifest of a `.wasm` file without installing it (to show its permissions).
   *  Rejects with kind `plugin` for anything that is not a valid plugin. */
  inspect(file: ImportSource): Promise<Manifest>;
  /** Installs (or updates) the plugin and grants the permissions its manifest requests; the
   *  UI asks the user first. Enabled after install. */
  install(file: ImportSource): Promise<PluginInfo>;
  remove(id: string): Promise<void>;
  setEnabled(id: string, enabled: boolean): Promise<void>;
  /** Renders a plugin tab for `target` (unsaved request state included). Collection tabs
   *  (wave 6) have `target.request_path` "". `environment`: the selected one, for the
   *  plugin's context (wave 6). */
  renderTab(
    plugin: string,
    tab: string,
    target: TabTarget,
    state: unknown,
    environment?: string | null,
  ): Promise<ViewReply>;
  /** Runs an action of a plugin tab (button, tree item, form submit with `input`). */
  action(
    plugin: string,
    tab: string,
    action: string,
    target: TabTarget,
    state: unknown,
    input: [string, string][],
    environment?: string | null,
  ): Promise<ViewReply>;
  /** The first-start question (wave 6): which bundled plugins to enable. Bundled plugins
   *  ship with the app and are installed but disabled until chosen. */
  welcome(): Promise<WelcomeState>;
  /** Answers the question: enables the chosen bundled plugins (`[]`: none, e.g. "REST
   *  only" or skipped) and never asks again. */
  chooseWelcome(plugins: string[]): Promise<void>;
}

/** OAuth 2.0 tokens of the open collection and the prompts of running flows. Tokens are
 *  fetched by `send` itself (auto fetch / refresh); `fetchToken` runs the flow on demand.
 *  Owner in wave 5: strand C. */
export interface AuthApi {
  /** Cached tokens of the open collection, without their values. */
  tokens(): Promise<TokenInfo[]>;
  /** Runs the OAuth 2.0 flow of the request at `path` now (prompts as needed). */
  fetchToken(path: string, input: SendInput): Promise<TokenInfo>;
  /** Forgets one cached token (`key`) or all of the collection (`null`). */
  clearTokens(key: string | null): Promise<void>;
  /** Called while a flow waits for the user (open browser, device code) and when it ends.
   *  Returns the unsubscribe function. */
  onPrompt(handler: (prompt: AuthPrompt) => void): () => void;
  /** Cancels the flow that is waiting for the user. */
  cancelPrompt(): Promise<void>;
}

/** The cookie jar of the open collection, per environment (`""` for none). Cookies from
 *  responses are stored and sent automatically (desktop; web through the companion only:
 *  the browser keeps its own). Owner in wave 5: strand D. */
export interface CookieApi {
  list(environment: string): Promise<CookieInfo[]>;
  delete(environment: string, domain: string, path: string, name: string): Promise<void>;
  clear(environment: string): Promise<void>;
}

/** Scripts of the open collection (wave 7). Scripts run only in a collection the user trusts
 *  (asked once per collection and device, like workspace trust); untrusted, a send runs
 *  without scripts and its report carries a warning note (`source: "script"`). Variables set
 *  by scripts, actions and "set as variable" live for the session (per environment) and go
 *  into the runtime layer of every later send; files are never written. Owner: strand A
 *  (trust), strand C (session variables). */
export interface ScriptsApi {
  trust(): Promise<ScriptTrust>;
  setTrusted(trusted: boolean): Promise<void>;
  /** The session's variables for `environment` (`""` for none), in the order set. */
  sessionVars(environment: string): Promise<VarChange[]>;
  /** Sets or unsets a session variable (e.g. a value clicked in the response). */
  setSessionVar(environment: string, change: VarChange): Promise<void>;
  clearSessionVars(environment: string): Promise<void>;
}

/** The collection runner (wave 7, strand D). One run at a time. */
export interface RunnerApi {
  /** Request paths below `folder` (`null`: the whole collection) in run order. */
  plan(folder: string | null): Promise<string[]>;
  /** Data rows of a CSV or JSON file; rejects with a message for invalid files. */
  parseData(file: ImportSource): Promise<DataRow[]>;
  /** Runs; `onEvent` sees progress. Variable changes stay in the run (and are not added to
   *  the session). Resolves with the summary, also when cancelled. */
  run(plan: RunPlan, input: SendInput, onEvent: (event: RunEvent) => void): Promise<RunSummary>;
  cancel(): Promise<void>;
}

/** Wave 8: sessions for WebSocket, gRPC and SSE requests (strands A and B). Desktop: native;
 *  web: the companion's `/v1/stream` (SSE may use `fetch`). Events are masked. */
export interface StreamsApi {
  /** Opens a session for the request at `path` (WebSocket, gRPC, or HTTP read as SSE);
   *  `onEvent` gets every event (`opened` first, `closed`/`error` last). Returns a session id. */
  open(path: string, input: SendInput, onEvent: (event: StreamEvent) => void): Promise<number>;
  /** Sends one message; `{{variables}}` are resolved, gRPC JSON is encoded to protobuf. */
  send(session: number, message: MessageVariant): Promise<void>;
  /** gRPC client/bidi streaming: no more messages from the client. */
  finish(session: number): Promise<void>;
  close(session: number): Promise<void>;
  /** The masked history so far (also after close, until `release`). */
  log(session: number): Promise<StreamLog>;
  release(session: number): Promise<void>;
}

/** Wave 8, strand B. */
export interface GrpcApi {
  /** Services of the request's proto file, the collection's `config.protobuf`, or (if
   *  neither) server reflection. */
  services(path: string, input: SendInput): Promise<GrpcService[]>;
  /** A JSON skeleton of the input message of `method` (path `/pkg.Service/Method`). */
  template(path: string, method: string, input: SendInput): Promise<string>;
}

/** Wave 8, strand B. */
export interface GraphQlApi {
  /** The schema by introspection, as SDL; cached per URL and environment until `refresh`. */
  schema(path: string, input: SendInput, refresh: boolean): Promise<string>;
}

/** Wave 8, strand C: what coding agents (`rt mcp`) may do; approvals; audit log. */
export interface AgentsApi {
  policy(): Promise<AgentPolicy>;
  /** Changes `extensions.requesttailor.agents` of `opencollection.yml` (unsaved, like edits). */
  setPolicy(policy: AgentPolicy): Promise<void>;
  /** Approvals `rt mcp` is waiting for (any collection on this machine). Web: always empty. */
  pending(): Promise<PendingApproval[]>;
  decide(id: string, decision: ApprovalDecision): Promise<void>;
  /** Called whenever the pending list changes; returns an unsubscribe function. */
  onPending(handler: (pending: PendingApproval[]) => void): () => void;
  /** Newest first. */
  audit(limit: number): Promise<AuditEntry[]>;
  /**
   * What the agent rules (the ones `rt mcp` applies) say about sending the request at `path`
   * in `environment` (`null`: none): its effect and the verdict of the collection's current
   * policy. Evaluated on the request as it is in memory, unsaved edits included, in the
   * active collection (`rt_app::agents::check`). Unknown request: kind `request-not-found`.
   * For extensions that send on a model's behalf: `refused` means do not send,
   * `needs-approval` means ask the user first.
   */
  check(path: string, environment: string | null): Promise<AgentCheck>;
  /**
   * The same rules for a request that is not saved in the collection (wave 19): one an
   * extension builds in memory, such as a probe of the API explorer
   * (`rt_app::agents::check_spec`). Never sends anything.
   */
  checkSpec(spec: RequestSpec, environment: string | null): Promise<AgentCheck>;
}

/** Wave 8, strand E: the AI assistant. Off until configured; keys never leave the keychain
 *  (web: encrypted browser storage) except in the provider call. */
export interface AiApi {
  config(): Promise<AiConfig | null>;
  setConfig(config: AiConfig | null): Promise<void>;
  hasKey(provider: AiProviderKind): Promise<boolean>;
  setKey(provider: AiProviderKind, key: string): Promise<void>;
  deleteKey(provider: AiProviderKind): Promise<void>;
  /** One task; the context is masked before it leaves. `input` selects the environment. */
  ask(task: AiTask, input: SendInput): Promise<AiAnswer>;

  // Wave 16 (strand A). The wave-8
  // methods above keep working: `config` is the default profile, `setKey` the provider's key.

  /** Profiles, roles and the preview mode; `null` until set up (a wave-8 config reads as one
   *  profile `default`). */
  settings(): Promise<AiSettings | null>;
  setSettings(settings: AiSettings | null): Promise<void>;
  /** A profile's own key (before the provider's key of `setKey`); never read back. */
  hasProfileKey(profile: string): Promise<boolean>;
  setProfileKey(profile: string, key: string): Promise<void>;
  deleteProfileKey(profile: string): Promise<void>;
  /** Whether the active collection allows the assistant (`opencollection.yml` and device). */
  policy(): Promise<AiCollectionPolicy>;
  /** Turns the assistant off (or on again) for the active collection on this device. */
  setDeviceOptOut(off: boolean): Promise<void>;
  /** Exactly what a task or a request would send, masked; nothing leaves. */
  preview(what: AiWhat, input: SendInput): Promise<AiPreview>;
  /** The user confirmed the first-use preview of `profile` on this device. */
  confirmPreview(profile: string): Promise<void>;
  /** A call of an extension: its prompt plus the core's masked context of `request.scope`,
   *  masked as a whole before it leaves. Rejects with kind `opted-out` when the collection
   *  is off and `cancelled` after `cancel(request.id)`. `onEvent` gets the streamed pieces. */
  complete(request: AiRequest, input: SendInput, onEvent?: (event: AiEvent) => void): Promise<AiReply>;
  /** Stops a running `complete`; nothing happens if it already ended. */
  cancel(id: string): Promise<void>;
}

/** Wave 16: what a preview is for. */
export type AiWhat = { kind: "task"; task: AiTask } | { kind: "request"; request: AiRequest };

/** Wave 8, strand D: testing MCP servers. `stdio` servers only on desktop. */
export interface McpClientApi {
  connect(server: McpServerConfig, input: SendInput): Promise<{ id: number; info: McpServerInfo }>;
  tools(id: number): Promise<McpTool[]>;
  call(id: number, tool: string, args: unknown): Promise<McpCallResult>;
  disconnect(id: number): Promise<void>;
  /** Saved tests (`*.mcp.yaml`), paths relative to the collection root. */
  tests(): Promise<string[]>;
  getTest(path: string): Promise<McpTest>;
  /** Stages a test file (written by `save`, like request edits); `path` null: a new file in
   *  `mcp/` named after the test. Returns the path. */
  saveTest(path: string | null, test: McpTest): Promise<string>;
  runTest(path: string, input: SendInput): Promise<McpTestResult>;
  /** Wave 14: resources of the server (empty without the `resources` capability). */
  resources(id: number): Promise<McpResource[]>;
  readResource(id: number, uri: string): Promise<McpResourceContents[]>;
  /** Wave 14: prompts of the server (empty without the `prompts` capability). */
  prompts(id: number): Promise<McpPrompt[]>;
  getPrompt(id: number, name: string, args: [string, string][]): Promise<McpPromptResult>;
  /** Wave 14, "Connect to RequestTailor itself": how this shell reaches `rt mcp` on the open
   *  collection. `server` is a stdio config when the shell can start it (desktop with `rt`
   *  found), else `null`; `hint` says in a sentence what happens or what to do instead. */
  selfServer(): Promise<McpSelfServer>;
  /** Wave 14: the demo server on a real loopback port, for other MCP clients (desktop);
   *  `null` in shells that cannot listen. The built-in demo (`MCP_DEMO_URL`) works everywhere. */
  readonly localDemo: McpLocalDemoApi | null;
}

/** The built-in demo server (rt_app::mcp_demo::DEMO_URL): answered in memory, never sent. */
export const MCP_DEMO_URL = "rt-demo://mcp";

/** The demo server on `127.0.0.1` with a random port, until stopped or the app ends. */
export interface McpLocalDemoApi {
  /** Its URL while running, else `null`. */
  status(): Promise<string | null>;
  /** Starts it (or returns the running one's URL). */
  start(): Promise<string>;
  stop(): Promise<void>;
}

/**
 * The collections in the sidebar (since wave 14, strand A; `rt_app::collections`). A folder the
 * user opens or adds (a *root*) gives one collection if it is one, else every collection in its
 * subfolders (depth 4, without hidden folders, `node_modules`, `target`); a folder without any
 * opens as a new collection. Roots are remembered per device like recent collections; no file
 * lists them. One collection is active: the per-collection commands of `Backend` act on it.
 */
export interface CollectionSetApi {
  /** The set as it stands; empty before anything is opened. */
  current(): Promise<CollectionSet>;
  /** Folder picker; adds the chosen folder as a root (its collections after the existing
   *  ones) and remembers it; `null` if cancelled. A folder already in the set is not added
   *  twice. */
  add(): Promise<CollectionSet | null>;
  /** Takes a root and its collections out of the set (files untouched). Rejects with kind
   *  `shell` while one of them has unsaved changes. */
  removeRoot(root: string): Promise<CollectionSet>;
  /** Makes collection `id` the active one. */
  activate(id: string): Promise<CollectionInfo>;
  /** The tree of any collection in the set, unsaved changes included. */
  tree(id: string): Promise<TreeNode>;
  /** Unsaved request paths of any collection in the set. */
  dirty(id: string): Promise<string[]>;
  /** Writes the unsaved changes of every collection; what was written, per collection. */
  saveAll(): Promise<CollectionChanges[]>;
  /** Full-text search over every collection, unsaved changes included, best hits first. */
  search(query: string, limit: number): Promise<CollectionSearchHit[]>;
  /** Creates a collection (folder with `opencollection.yml`, optional first environment and
   *  `baseUrl`), unsaved like any edit until `save`/`saveAll`, adds it to the set and makes
   *  it active. `spec.root: null` asks for a folder first (`null` if cancelled). */
  create(spec: NewCollection): Promise<CollectionInfo | null>;
  /** Shows the folder of collection `id` in the platform's file manager ("Show in Finder").
   *  Only shells that can do that have it (desktop); the sidebar offers it where it exists. */
  reveal?(id: string): Promise<void>;
}

export interface Backend {
  readonly kind: "tauri" | "web" | "mock";

  /** Wave 14: the collections in the sidebar. */
  readonly collections: CollectionSetApi;
  /** Wave 15: repositories in the set, also without a collection. */
  readonly repositories: RepositoryApi;

  /** Shows the platform's folder picker and opens what it holds as the only root of the
   *  collection set (wave 14: one collection, or every collection in its subfolders); returns
   *  the active collection; `null` if cancelled. */
  openCollection(): Promise<CollectionInfo | null>;
  /** Collections opened on this device, most recent first. Owner in wave 4: strand C. */
  recentCollections(): Promise<RecentCollection[]>;
  /** Opens a recent collection without a picker. `interactive: false` (at start) never
   *  prompts and resolves `null` if the platform needs the user's consent first (the browser's
   *  folder permission); `interactive: true` runs from a click and may prompt. Also `null` if
   *  the folder is gone. */
  openRecent(id: string, interactive: boolean): Promise<CollectionInfo | null>;
  tree(): Promise<TreeNode>;
  getRequest(path: string): Promise<RequestSpec>;
  /** New GET request named `name` in `folder` (`""` = root). Returns its path. Unsaved. */
  createRequest(folder: string, name: string): Promise<string>;
  /** Makes sure folder `path` exists (`/`-separated; each segment matches a folder by
   *  directory or display name, or is created). Returns the folder's path for
   *  `createRequest`. Unsaved. Wave 6 (plugin `create_requests`). */
  createFolder(path: string): Promise<string>;
  /** Replaces a request in memory. Unsaved until `save`. */
  updateRequest(path: string, spec: RequestSpec): Promise<void>;
  /** Request paths with unsaved changes. */
  dirty(): Promise<string[]>;
  /** Writes all unsaved changes to the collection's files and returns what was written. */
  save(): Promise<FileChange[]>;
  /** Environments as in the files; secret variables have `value: null`. */
  environments(): Promise<Environment[]>;
  /** Creates or replaces an environment by name (unsaved until `save`). Secret values in
   *  `env` are ignored: set them with `secrets.set`. */
  updateEnvironment(env: Environment): Promise<void>;
  deleteEnvironment(name: string): Promise<void>;

  /** Resolves the request as `send` would, without sending (variable provenance, preview). */
  preview(path: string, input: SendInput): Promise<Preview>;

  /** Sends the saved-or-unsaved current state of the request. Secrets are added by the shell. */
  send(path: string, input: SendInput): Promise<StoredResponse>;
  responseLines(id: number, start: number, count: number): Promise<LineSlice>;
  responseBytes(id: number, offset: number, length: number): Promise<Uint8Array>;
  /** JSONPath over a JSON body; returns pretty JSON. Rejects with a message on error. */
  filterJson(id: number, expression: string): Promise<string>;
  releaseResponse(id: number): Promise<void>;

  /** Full-text search over the open collection, unsaved changes included. Owner in wave 4:
   *  strand E. */
  search(query: string, limit: number): Promise<SearchHit[]>;

  readonly secrets: SecretsApi;
  readonly transport: TransportApi;
  readonly git: GitApi;
  readonly imports: ImportApi;
  /** OpenAPI import wizard and run wizard. */
  readonly openapi: OpenApiApi;
  readonly history: HistoryApi;
  /** Wave 5. */
  readonly plugins: PluginApi;
  readonly auth: AuthApi;
  readonly cookies: CookieApi;
  /** Wave 7. */
  readonly scripts: ScriptsApi;
  readonly runner: RunnerApi;
  /** Wave 8. */
  readonly streams: StreamsApi;
  readonly grpc: GrpcApi;
  readonly graphql: GraphQlApi;
  readonly agents: AgentsApi;
  readonly ai: AiApi;
  readonly mcp: McpClientApi;
  /** Wave 12 (Git for APIs), one file per strand in each shell. */
  readonly diffs: DiffApi;
  readonly merges: MergeApi;
  readonly compare: CompareApi;
  readonly timeline: TimelineApi;
  readonly review: ReviewApi;
  readonly contract: ContractApi;
  readonly rescue: RescueApi;
  /** Wave 13: companion files for extensions. */
  readonly companions: CompanionApi;
  /** Wave 14: variable completion. */
  readonly variables: VariablesApi;
  /** Wave 16: the scripts of the collection and its folders (suggested edits). */
  readonly folderScripts: FolderScriptsApi;
  /** Wave 18: comparing environments, response checks, paging, the webhook inbox, lint, saved
   *  views and the plugin catalogue. */
  readonly envCompare: EnvCompareApi;
  readonly inspect: InspectApi;
  readonly paging: PagingApi;
  readonly inbox: InboxApi;
  readonly lint: LintApi;
  readonly views: ViewsApi;
  readonly catalogue: CatalogueApi;
  /** Wave 19: evidence and cleaning up. */
  readonly egress: EgressApi;
  readonly journal: JournalApi;
  readonly access: AccessApi;
  readonly shrink: ShrinkApi;
  readonly consumers: ConsumersApi;
  readonly sequence: SequenceApi;
  readonly security: SecurityApi;
  readonly notebook: NotebookApi;
  readonly debugger: DebuggerApi;
  /** Wave 17: snapshot tests (`rt_app::snapshots`) and sharing (`rt_app::share`). */
  readonly snapshots: SnapshotApi;
  readonly share: ShareApi;
}

/**
 * Snapshot tests (wave 17, strand B). A snapshot is a companion file next to the request
 * (`<request name>.snap.json`) plus one `matchesSnapshot` assertion in the request; both are
 * staged unsaved like any edit (`Backend.companions`, `updateRequest`). `responseId` is a
 * stored response of the request (`StoredResponse.response_id`). Secret values never reach a
 * snapshot.
 */
export interface SnapshotApi {
  /** The snapshot of the request at `path`; `null` if it has none. */
  read(path: string): Promise<Snapshot | null>;
  /** Freezes the stored response as the request's snapshot (and adds the assertion if it has
   *  none). Freezing again accepts a new response. Rejects with kind `secrets-found` when the
   *  scanner flags the body. */
  freeze(path: string, responseId: number, options?: FreezeOptions): Promise<SnapshotFreeze>;
  /** Replaces the snapshot's ignored paths. */
  setIgnore(path: string, ignore: string[]): Promise<Snapshot>;
  /** Compares the stored response with the snapshot. */
  check(path: string, responseId: number): Promise<SnapshotCheck>;
  /** Every request of the active collection that has a snapshot assertion. */
  list(): Promise<SnapshotEntry[]>;
}

/**
 * Sharing without a server and copying as code (wave 17, strand C). Nothing is uploaded and
 * no request is made; secret values are never in a link or a snippet.
 */
export interface ShareApi {
  /** A link (or payload text) for the request at `path`. Rejects with kind `secrets-found`
   *  when the scanner finds credentials that cannot be replaced, and with kind `format`
   *  when the request is too large for a link. */
  link(path: string, options?: ShareOptions): Promise<ShareLink>;
  /** Reads a link, a fragment (`#rt=…`) or an `rt:` payload. Nothing runs and nothing is
   *  created: the app opens the result as an unsaved draft (`AppState.openShared`). */
  open(input: string): Promise<SharePayload>;
  /** The request as curl, `fetch` or Python; variables stay variables of the target tool. */
  copyAs(path: string, kind: CopyKind, options?: CopyOptions): Promise<CopiedAs>;
  /** The link as an SVG QR code. Rejects when it is too long for one. */
  qr(link: string): Promise<string>;
}

/**
 * Scripts of the collection (`path` is `""`) or of a folder (since wave 16,
 * `rt_app::Workspace::{folder_scripts, set_folder_scripts}`).
 */
export interface FolderScriptsApi {
  /** The scripts in file order (phases RequestTailor does not run, gRPC, are not listed).
   *  Unknown folder: kind `folder-not-found`. */
  get(path: string): Promise<Script[]>;
  /** Replaces them. Unsaved, like any edit (`dirty` lists the file); the edit is read back
   *  and refused if the file cannot hold it. */
  set(path: string, scripts: Script[]): Promise<void>;
}

/**
 * Variable completion (since wave 14, `rt_app::variables`): the `{{` list of the editor.
 */
export interface VariablesApi {
  /** Every variable `path` sees: `path` is a request path, a folder path, or `""` for the
   *  collection (environment values). With the session's variables in the runtime layer, like
   *  `preview`; secret values never (secrets have `value: null`). */
  scope(path: string, input: SendInput): Promise<VariableScope>;
  /** Adds a plain variable to the collection (`folder` is `""`) or a folder, or replaces the
   *  one of that name there. Unsaved, like any edit (`dirty` lists the file). Secret
   *  variables are refused (kind `unsupported`): they belong in an environment. */
  define(folder: string, variable: Variable): Promise<void>;
}

/**
 * Companion files of the open collection (since wave 13): files the OpenCollection document
 * does not own (snapshots, recordings, notes), for extensions. Text only. Writes are staged
 * and saved with `save`, like a request edit (`dirty` lists them until then). Collection
 * files (`*.yml`) and paths outside the folder are refused (kind `shell`).
 */
export interface CompanionApi {
  /** Paths below `prefix` (`""`: all; a folder path, with or without `/`), unsaved ones
   *  included, sorted; never `*.yml`. */
  list(prefix: string): Promise<string[]>;
  /** The text (unsaved first); `null` if there is no such file. */
  read(path: string): Promise<string | null>;
  stage(path: string, text: string): Promise<void>;
}

// ---- wave 18 ----

/** Comparing environments (wave 18, strand A; `rt_app::envcompare`). */
export interface EnvCompareApi {
  run(request: EnvCompareRequest): Promise<EnvComparison>;
}

/** Response checks (wave 18, strand B; `rt_app::inspect`). */
export interface InspectApi {
  jwt(token: string): Promise<JwtInfo>;
  tokens(target: InspectTarget): Promise<FoundToken[]>;
  checks(target: InspectTarget): Promise<Observation[]>;
  ignoreCheck(id: string, scope: CheckScope): Promise<void>;
}

/** Paging (wave 18, strand C; `rt_app::paging`). */
export interface PagingApi {
  /** The rule in force; with the kept response `responseId`, a plugin's rule is matched on its
   *  URL and content type (without it on the request as written). */
  rule(path: string, responseId?: number | null): Promise<EffectivePaging | null>;
  setRule(path: string, spec: PagingSpec | null, scope: PagingRuleScope): Promise<void>;
  run(run: PagingRun): Promise<PagedResult>;
}

/** The webhook inbox (wave 18, strand E; `rt_app::inbox`). */
export interface InboxApi {
  settings(): Promise<InboxSettings>;
  setSettings(settings: InboxSettings): Promise<void>;
  start(): Promise<InboxStatus>;
  stop(): Promise<InboxStatus>;
  status(): Promise<InboxStatus>;
  events(since: number): Promise<InboxPage>;
  get(id: number): Promise<CapturedRequest | null>;
  clear(): Promise<void>;
  makeRequest(id: number): Promise<InboxDraft>;
}

/** Lint (wave 18, strand F; `rt_app::lint`). */
export interface LintApi {
  run(): Promise<LintReport>;
  settings(): Promise<LintSettings>;
  setSettings(settings: LintSettings): Promise<void>;
  ignore(path: string, rule: LintRule): Promise<void>;
}

/** Saved views (wave 18, strand G; `rt_app::views`). */
export interface ViewsApi {
  list(path: string): Promise<ResponseView[]>;
  save(path: string, view: ResponseView): Promise<ResponseView[]>;
  remove(path: string, name: string): Promise<ResponseView[]>;
  apply(path: string, responseId: number, view: ResponseView): Promise<ViewResult>;
  languages(): Promise<ViewLanguage[]>;
}

/** The plugin catalogue (wave 18, strand I; `rt_app::catalogue`). */
export interface CatalogueApi {
  sources(): Promise<CatalogueSource[]>;
  addSource(url: string): Promise<CatalogueSource>;
  removeSource(url: string): Promise<void>;
  /** Reads the index of one source, or of every source (nothing is read before this call). */
  refresh(source?: string): Promise<SourceIndex[]>;
  plan(source: string, id: string): Promise<InstallPlan>;
  install(plan: InstallPlan): Promise<InstalledSource>;
  installed(): Promise<InstalledSource[]>;
  checkUpdates(): Promise<CatalogueUpdate[]>;
}

// ---- wave 19 ----

/** Guided network access for extensions (wave 19, strand B; `rt_app::egress`). */
export interface EgressApi {
  hosts(): Promise<EgressHost[]>;
  allow(host: string, port: number | null, extension: string, purpose: string): Promise<EgressHost>;
  remove(host: string, port: number | null): Promise<void>;
  request(request: EgressRequest): Promise<EgressResponse>;
}

/** The test-data journal and reverting (wave 19, strand C; `rt_app::journal`). */
export interface JournalApi {
  settings(): Promise<JournalSettings>;
  setSettings(settings: JournalSettings): Promise<void>;
  entries(query: JournalQuery): Promise<JournalPage>;
  clear(): Promise<void>;
  planManual(revert: ManualRevert): Promise<RevertPlan>;
  planCompensations(items: CompensationItem[]): Promise<RevertPlan>;
  execute(plan: RevertPlan, options: RevertOptions): Promise<RevertOutcome>;
  pluginRules(): Promise<PluginCompensationRule[]>;
  preRead(request: RequestSpec, environment: string | null): Promise<PreRead>;
  states(entryIds: number[]): Promise<EntryStates[]>;
}

/** The two-user access check of one request (wave 19, strand D; `rt_app::access`). */
export interface AccessApi {
  check(path: string, profile: AccessProfile, options: AccessOptions): Promise<AccessCheck>;
}

/** Minimal failing request (wave 19, strand E; `rt_app::shrink`). */
export interface ShrinkApi {
  run(request: ShrinkRequest): Promise<ShrinkResult>;
  cancel(): Promise<void>;
  predicate(path: string, responseId: number | null): Promise<ShrinkPredicate>;
}

/** Consumer contracts without a broker (wave 19, strand F; `rt_app::consumers`). */
export interface ConsumersApi {
  list(): Promise<ConsumerRef[]>;
  verify(request: ConsumerVerifyRequest): Promise<ConsumerReport>;
}

/** Sequence diagrams from runs (wave 19, strand G; `rt_app::sequence`). */
export interface SequenceApi {
  model(source: SequenceSource, options: SequenceOptions): Promise<SequenceModel>;
  render(model: SequenceModel, options: SequenceOptions): Promise<SequenceText>;
  clear(): Promise<void>;
}

/** Passive API security checks (wave 19, strand R; `rt_app::security`). */
export interface SecurityApi {
  check(target: SecurityTarget): Promise<SecurityFinding[]>;
  rules(): Promise<SecurityRule[]>;
  settings(): Promise<SecuritySettings>;
  setSettings(settings: SecuritySettings): Promise<void>;
}

/** API notebooks (wave 19, strand W; `rt_app::notebook`). */
export interface NotebookApi {
  list(): Promise<NotebookEntry[]>;
  read(path: string): Promise<Notebook>;
  stage(path: string, notebook: Notebook): Promise<void>;
  run(run: NotebookRun): Promise<NotebookOutcome>;
}

/** The script debugger (wave 19, strand Y; `rt_script::debug`). */
export interface DebuggerApi {
  start(target: DebugTarget): Promise<DebugState>;
  command(session: number, command: DebugCommand): Promise<DebugState>;
  state(session: number): Promise<DebugState>;
  stop(session: number): Promise<void>;
}

