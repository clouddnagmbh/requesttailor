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
  GitBranch,
  GitCommit,
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
  /** Clones `url` into a new folder inside one the user picks and opens it; `null` if
   *  cancelled. */
  clone(url: string): Promise<CollectionInfo | null>;
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
  /** `null` if the user cancelled the folder picker (the plan stays available). */
  write(id: number): Promise<CollectionInfo | null>;
  discard(id: number): Promise<void>;
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

export interface Backend {
  readonly kind: "tauri" | "web" | "mock";

  /** Shows the platform's folder picker and opens the collection; `null` if cancelled. */
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
  readonly history: HistoryApi;
  /** Wave 5. */
  readonly plugins: PluginApi;
  readonly auth: AuthApi;
  readonly cookies: CookieApi;
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
