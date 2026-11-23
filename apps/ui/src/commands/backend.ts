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

import type {
  Environment,
  Script,
  FileChange,
  GitBranch,
  GitAction,
  GitActionResult,
  GitChangedFile,
  GitCommit,
  GitGraphPage,
  GitGraphQuery,
  GitPlan,
  GitRef,
  GitRemote,
  GitStash,
  GitUndoEntry,
  GitConflict,
  GitDiff,
  GitIdentity,
  GitPullResult,
  GitResolution,
  GitStatus,
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
  SecretFinding,
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
  CiStatus,
  CollectionDiff,
  CollectionChanges,
  CollectionSearchHit,
  CollectionSet,
  NewCollection,
  CompareRequest,
  ConflictChoice,
  ContractReport,
  ContractSpec,
  DeviceCodePrompt,
  FieldBlame,
  GitFileCommit,
  GitTree,
  HistoryScan,
  MergeState,
  NewReviewComment,
  PullRequest,
  RequestTimeline,
  RescuePlan,
  RescueResult,
  ReviewComment,
  ReviewHost,
  ReviewRepo,
  ReviewSession,
  ReviewSubmission,
  RevisionRun,
  RevisionRunRequest,
  RunComparison,
  ScanProgress,
  SpecChanges,
  TestRecord,
  Variable,
  VariableScope,
} from "./types";

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

/** Git for the repository that holds the open collection (rt_app::git describes the model:
 *  no staging area, paths relative to the collection). Operations that change the working
 *  tree (`checkout`, `pull`, `resolve`) reject with kind `git` while there are unsaved
 *  changes; afterwards the UI calls `AppState.reload()`. Remote operations reject with kind
 *  `git-auth` (message: the host) when credentials are missing or wrong.
 *  Web: `push`, `pull` and `clone` go through the companion and reject with kind
 *  `unsupported` without it. Owners in wave 4: strands A1 (desktop) and A2 (web, UI). */
export interface GitApi {
  status(): Promise<GitStatus>;
  /** `HEAD` → working tree of one file. */
  diff(path: string): Promise<GitDiff>;
  /** Commits exactly `paths` (new, changed or deleted) with `message`. */
  commit(message: string, paths: string[]): Promise<GitCommit>;
  /** Commits of the current branch, newest first. */
  log(limit: number): Promise<GitCommit[]>;
  branches(): Promise<GitBranch[]>;
  /** Switches to a local branch, or to a remote one (`origin/x` → local `x` tracking it). */
  checkout(branch: string): Promise<void>;
  createBranch(name: string, checkout: boolean): Promise<void>;
  /** Pushes the current branch to its upstream (sets `origin/<branch>` if it has none). */
  push(): Promise<void>;
  /** Fetches and merges the upstream into the current branch. */
  pull(): Promise<GitPullResult>;
  conflict(path: string): Promise<GitConflict>;
  resolve(path: string, resolution: GitResolution): Promise<void>;
  /** Creates a repository in the collection folder. */
  init(): Promise<void>;
  /** Clones `url` (HTTPS, or on desktop a local repository path / `file://` URL) into a new folder inside one the user picks and opens it; `null` if
   *  cancelled. */
  clone(url: string): Promise<CollectionInfo | null>;
  /** Desktop only (absent elsewhere): shows the folder picker for a repository on this
   *  machine and returns its path for the clone field; `null` if cancelled. `clone` also
   *  accepts the path forms `~/…`, `file://…` and relative paths. */
  pickRepository?(): Promise<string | null>;
  /** Author for commits; `null` if none is configured. */
  identity(): Promise<GitIdentity | null>;
  setIdentity(identity: GitIdentity): Promise<void>;
  /** HTTPS credentials per host, kept like secret values (keychain / encrypted storage). */
  hasCredentials(host: string): Promise<boolean>;
  setCredentials(host: string, username: string, password: string): Promise<void>;
  deleteCredentials(host: string): Promise<void>;
  /** Credentials typed into `paths` (as they are on disk). `commit` runs the same scan and
   *  rejects with kind `secrets-found` when it finds any. Owner in wave 5: strand E. */
  scan(paths: string[]): Promise<SecretFinding[]>;

  // Wave 12 (interface): revisions, read without a checkout. A `rev` is a full or abbreviated
  // commit id, a branch (`main`, `origin/main`), a ref (`refs/...`) or `HEAD`, optionally
  // followed by `~<n>` or `^`. Paths are relative to the collection root.
  /** The commit `rev` names (full id). Rejects with kind `git` for an unknown rev. */
  revParse(rev: string): Promise<string>;
  /** The collection's files at `rev`, as the shells read them from disk. */
  treeAt(rev: string): Promise<GitTree>;
  /** Text of `path` at `rev`, `null` if it does not exist there. A leading `/` makes the path
   *  relative to the repository root (the web: the collection folder is the root). */
  show(rev: string, path: string): Promise<string | null>;
  /** Commits of `HEAD`'s history that changed `path`, newest first (renames: strand D). */
  fileLog(path: string, limit: number): Promise<GitFileCommit[]>;
  /** Best common ancestor; `null` if the histories are unrelated. */
  mergeBase(a: string, b: string): Promise<string | null>;
  /** Commits reachable from `to` along first parents but not from `from` (`null`: down to
   *  the root), newest first. */
  commits(from: string | null, to: string, limit: number): Promise<GitCommit[]>;

  // Wave 14 (interface): Git for the whole repository of the active collection, generic (no
  // assumption that it is a collection). Paths repository-relative with a leading `/`.
  // Strand F: the commit graph and what a commit changed.
  /** A page of the commit graph (all refs, topological, newest first). */
  graph(query: GitGraphQuery): Promise<GitGraphPage>;
  /** All branches, remote branches, tags and a detached `HEAD`, with ahead/behind. */
  refs(): Promise<GitRef[]>;
  /** Files changed from `from` to `to` (`null`: `to` against its first parent). */
  changes(from: string | null, to: string): Promise<GitChangedFile[]>;
  /** Line diff of any file between two revs (`to` `null`: the working tree). */
  fileDiff(from: string | null, to: string | null, path: string): Promise<GitDiff>;
  // Strand G: the wizards. `plan` only reads; `apply` plans again and rejects with kind `git`
  // and `GitPlan.blocked.message` when blocked, kind `git-auth` when credentials are needed.
  remotes(): Promise<GitRemote[]>;
  stashes(): Promise<GitStash[]>;
  plan(action: GitAction): Promise<GitPlan>;
  apply(action: GitAction): Promise<GitActionResult>;
  /** The newest entry of the action journal, `null` if there is none. */
  lastAction(): Promise<GitUndoEntry | null>;
  /** Takes the entry `id` back (refs, working tree); rejects when it is `stale`. */
  undo(id: string): Promise<GitActionResult>;
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

/** Pull requests (GitHub) and merge requests (GitLab) of the collection's `origin`, with the
 *  user's own token (keychain / encrypted browser storage). Owner in wave 12: strand E. */
export interface ReviewApi {
  /** The Git host and project of `origin`; `null` if it is none the app knows. */
  repo(): Promise<ReviewRepo | null>;
  /** Self-hosted Git hosts the user added (GitHub Enterprise, GitLab), per device;
   *  `github.com` and `gitlab.com` need none. */
  hosts(): Promise<ReviewHost[]>;
  setHosts(hosts: ReviewHost[]): Promise<void>;
  hasToken(host: string): Promise<boolean>;
  setToken(host: string, token: string): Promise<void>;
  deleteToken(host: string): Promise<void>;
  /** Device-code login: the code to show; then `finishLogin` waits for the user. */
  startLogin(host: string): Promise<DeviceCodePrompt>;
  finishLogin(host: string): Promise<void>;
  list(): Promise<PullRequest[]>;
  /** Fetches the PR's head into a local ref (no checkout) for diff and runs. */
  open(number: number): Promise<ReviewSession>;
  comments(number: number): Promise<ReviewComment[]>;
  comment(number: number, comment: NewReviewComment): Promise<ReviewComment>;
  submit(number: number, review: ReviewSubmission): Promise<void>;
  ci(number: number): Promise<CiStatus>;
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
