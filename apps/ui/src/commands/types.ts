// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// TypeScript mirror of the serde JSON of rt_core / rt_app types. Kept in sync by the contract
// test: `cargo test -p rt-app --test contract` writes `contract.generated.ts`, which annotates
// real serialized values with these types, so `npm run check` fails on any drift.
// Changing these types is an interface change (working rule 1).

// ---- model (rt_core::model) ----

export interface KeyValue {
  name: string;
  value: string;
  enabled: boolean;
}

export type ParamKind = "query" | "path";

/** `query` params only mirror the URL's query string; the URL is the truth. */
export interface Param {
  name: string;
  value: string;
  kind: ParamKind;
  enabled: boolean;
}

export type RawKind = "json" | "text" | "xml" | "sparql";
export type PartKind = "text" | "file";

export interface MultipartPart {
  name: string;
  kind: PartKind;
  values: string[];
  content_type: string | null;
  enabled: boolean;
}

export type Body =
  | { type: "none" }
  | { type: "raw"; kind: RawKind; data: string }
  | { type: "form-url-encoded"; fields: KeyValue[] }
  | { type: "multipart"; parts: MultipartPart[] }
  | { type: "file"; path: string; content_type: string | null };

export type ApiKeyPlacement = "header" | "query";

export type Auth =
  | { type: "inherit" }
  | { type: "none" }
  | { type: "basic"; username: string; password: string }
  | { type: "bearer"; token: string }
  | { type: "api-key"; key: string; value: string; placement: ApiKeyPlacement }
  | ({ type: "oauth2" } & OAuth2)
  | ({ type: "awsv4" } & AwsSigV4)
  | { type: "unsupported"; kind: string };

// ---- auth (rt_core::model::auth, wave 5) ----

export type PkceMethod = "S256" | "plain";
export type CredentialsPlacement = "basic-auth-header" | "body";
export type OAuth2Step = "authorization" | "access-token" | "refresh";
export type ParamPlacement = "header" | "query" | "body";
export type TokenSource = "access-token" | "id-token";

export type OAuth2Flow =
  | { flow: "client-credentials" }
  | { flow: "password"; username: string; password: string }
  | {
      flow: "authorization-code";
      authorization_url: string;
      callback_url: string;
      state: string;
      /** `null`: PKCE disabled. */
      pkce: PkceMethod | null;
    }
  | { flow: "implicit"; authorization_url: string; callback_url: string; state: string }
  /** Collection default only. */
  | { flow: "device-code"; device_authorization_url: string }
  /** Collection default only. */
  | { flow: "saml2-bearer"; assertion: string };

export interface OAuth2Param {
  step: OAuth2Step;
  name: string;
  value: string;
  placement: ParamPlacement;
}

export type TokenPlacement = { in: "header"; prefix: string } | { in: "query"; name: string };

export interface TokenUse {
  id: string;
  placement: TokenPlacement;
  source: TokenSource;
}

/** The flow's fields are flattened into the same object (`flow` is the tag). */
export type OAuth2 = OAuth2Flow & {
  access_token_url: string;
  refresh_token_url: string;
  client_id: string;
  client_secret: string;
  credentials_placement: CredentialsPlacement;
  scope: string;
  parameters: OAuth2Param[];
  token: TokenUse;
  auto_fetch: boolean;
  auto_refresh: boolean;
};

export interface AwsSigV4 {
  access_key_id: string;
  secret_access_key: string;
  session_token: string;
  service: string;
  region: string;
  profile_name: string;
}

export type CertificateSource =
  | { type: "pem"; certificate_path: string; private_key_path: string }
  | { type: "pkcs12"; path: string };

export interface ClientCertificate {
  domain: string;
  source: CertificateSource;
  passphrase: string;
  enabled: boolean;
}

export type SecretProvider =
  | "hashicorp-vault-cloud"
  | "hashicorp-vault-server"
  | "aws-secrets-manager"
  | "azure-key-vault"
  | "gcp-secrets-manager"
  | "one-password";

export interface ExternalSecret {
  name: string;
  /** Vault path, AWS secret name, Azure vault name, GCP project id, or `op://…`. */
  reference: string;
  enabled: boolean;
}

export interface ExternalSecrets {
  provider: SecretProvider;
  variables: ExternalSecret[];
}

export interface Settings {
  timeout_ms: number | null;
  follow_redirects: boolean | null;
  max_redirects: number | null;
  encode_url: boolean | null;
}

export interface Variable {
  name: string;
  /** `null` for secrets (the value lives in the secret store) and value-less variables. */
  value: string | null;
  enabled: boolean;
  secret: boolean;
}

export interface Environment {
  name: string;
  variables: Variable[];
  client_certificates: ClientCertificate[];
  external_secrets: ExternalSecrets | null;
}

export interface RequestSpec {
  name: string;
  seq: number | null;
  method: string;
  url: string;
  headers: KeyValue[];
  params: Param[];
  body: Body;
  auth: Auth;
  vars: Variable[];
  settings: Settings;
}

// ---- variables (rt_core::vars) ----

export type VarSource =
  | { layer: "global" }
  | { layer: "collection" }
  | { layer: "environment"; name: string }
  | { layer: "folder"; path: string }
  | { layer: "request" }
  | { layer: "runtime" };

export interface VarUse {
  name: string;
  source: VarSource;
  secret: boolean;
}

// ---- send report (rt_core::send, rt_core::transport) ----

export type HttpVersion = "HTTP/1.0" | "HTTP/1.1" | "HTTP/2" | "HTTP/3" | "unknown";

/** Durations are milliseconds (fractional). Phases a transport cannot see are `null`. */
export interface Timing {
  dns: number | null;
  connect: number | null;
  tls: number | null;
  ttfb: number;
}

export interface Redirect {
  status: number;
  location: string;
}

export type TransportError =
  | { kind: "invalid-url"; message: string }
  | { kind: "dns"; message: string }
  | { kind: "connect"; message: string }
  | { kind: "tls"; message: string }
  | { kind: "timeout" }
  | { kind: "too-many-redirects"; message: number }
  /** The browser refused the request (CORS, mixed content, network). Offer the companion. */
  | { kind: "blocked"; message: string }
  | { kind: "other"; message: string };

export type SendError =
  | { kind: "not-found"; message: string }
  | { kind: "prepare"; message: string }
  | { kind: "auth"; message: string }
  | { kind: "plugin"; message: string }
  | { kind: "transport"; message: TransportError };

export type NoteLevel = "info" | "warning" | "error";

/** From the auth step (`source: "auth"`) or a plugin (`"plugin:<id>"`). */
export interface Note {
  source: string;
  level: NoteLevel;
  text: string;
}

export interface SentRequest {
  method: string;
  url: string;
  /** Secret values masked as `***`. */
  headers: [string, string][];
  body_size: number;
}

export type ReportBody = { encoding: "utf8"; data: string } | { encoding: "base64"; data: string };

export interface ReceivedResponse {
  status: number;
  version: HttpVersion;
  headers: [string, string][];
  url: string;
  redirects: Redirect[];
  body: ReportBody;
  size: number;
  truncated: boolean;
  timing: Timing;
  total: number;
}

export interface SendReport {
  version: number;
  request_path: string;
  environment: string | null;
  request: SentRequest | null;
  response: ReceivedResponse | null;
  variables: VarUse[];
  unresolved: string[];
  error: SendError | null;
  /** Omitted when empty. */
  notes?: Note[];
}

// ---- app (rt_app) ----

export type TreeNode =
  | { kind: "folder"; name: string; path: string; children: TreeNode[] }
  | { kind: "request"; name: string; path: string; method: string };

export interface PreviewVar {
  name: string;
  /** `null` for secrets (never shown resolved) and unresolved names. */
  value: string | null;
  source: VarSource;
  secret: boolean;
}

/** What a request would send, computed by the core without sending. Secrets masked. */
export interface Preview {
  url: string | null;
  headers: [string, string][];
  variables: PreviewVar[];
  unresolved: string[];
  error: string | null;
}

export interface FileChange {
  path: string;
  /** Empty when `deleted`. */
  text: string;
  /** Delete the file (a file that is already gone is fine). */
  deleted: boolean;
}

export type AppError =
  | { kind: "format"; message: string }
  | { kind: "request-not-found"; message: string }
  | { kind: "folder-not-found"; message: string }
  | { kind: "response-not-found"; message: number }
  | { kind: "environment-not-found"; message: string }
  | { kind: "filter"; message: string }
  | { kind: "import"; message: string }
  | { kind: "git"; message: string }
  /** The remote wants credentials or rejected them; `message` is the host. */
  | { kind: "git-auth"; message: string }
  | { kind: "plugin"; message: string }
  | { kind: "auth"; message: string }
  /** A commit was refused: files contain credentials (see `GitApi.scan`). */
  | { kind: "secrets-found"; message: string }
  | { kind: "unsupported"; message: string }
  | { kind: "shell"; message: string };

export interface StoredResponse {
  /** `report.response.body` is a preview of at most 64 KiB; read the rest by lines. */
  report: SendReport;
  response_id: number | null;
  size: number;
  line_count: number;
  is_text: boolean;
}

export interface LineSlice {
  start: number;
  lines: string[];
  /** Indexes (relative to `start`) of lines cut at 10 000 characters. */
  cut: number[];
}

export interface SecretKey {
  collection: string;
  environment: string;
  variable: string;
}

// ---- git (rt_app::git) ----

export type GitFileState = "untracked" | "modified" | "deleted" | "conflicted";

export interface GitFile {
  path: string;
  state: GitFileState;
}

export interface GitStatus {
  /** `false`: the collection is not in a Git repository. */
  repo: boolean;
  branch: string | null;
  upstream: string | null;
  ahead: number;
  behind: number;
  files: GitFile[];
  merging: boolean;
}

export interface GitBranch {
  name: string;
  current: boolean;
  remote: boolean;
}

export interface GitCommit {
  oid: string;
  summary: string;
  author: string;
  /** Unix seconds. */
  time: number;
}

export type GitPullResult =
  | { outcome: "up-to-date" }
  | { outcome: "fast-forward"; commits: number }
  | { outcome: "merged"; oid: string }
  | { outcome: "conflicts"; paths: string[] };

export interface GitConflict {
  path: string;
  base: string | null;
  ours: string | null;
  theirs: string | null;
}

export type GitResolution = "ours" | "theirs";

export interface GitIdentity {
  name: string;
  email: string;
}

export type DiffLineKind = "context" | "added" | "removed";

export interface DiffLine {
  kind: DiffLineKind;
  text: string;
}

export interface DiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: DiffLine[];
}

export interface GitDiff {
  path: string;
  binary: boolean;
  hunks: DiffHunk[];
}

// ---- import (rt_format::import, rt_app::import) ----

export type ImportFormat = "postman" | "insomnia" | "openapi" | "curl" | "har" | "http" | "plugin";

export type LossKind =
  | "script"
  | "test"
  | "auth"
  | "body"
  | "variable"
  | "setting"
  | "documentation"
  | "example"
  | "protocol"
  | "other";

export type LossEffect = "dropped" | "approximated";

export interface Loss {
  /** Where in the source, e.g. `Users / Get user`. */
  location: string;
  kind: LossKind;
  effect: LossEffect;
  detail: string;
}

export interface LossReport {
  format: ImportFormat;
  requests: number;
  folders: number;
  environments: number;
  losses: Loss[];
}

export interface ImportPlan {
  id: number;
  format: ImportFormat;
  collection_name: string;
  folder_name: string;
  files: number;
  report: LossReport;
}

// ---- history and search (rt_app::history, rt_app::search) ----

export interface HistoryEntry {
  id: number;
  /** Unix milliseconds. */
  at: number;
  collection: string;
  request_path: string;
  name: string;
  environment: string | null;
  method: string;
  /** As sent, secrets masked. */
  url: string | null;
  status: number | null;
  error: SendError | null;
  duration_ms: number | null;
  size: number | null;
}

export type SearchField = "name" | "url" | "param" | "header" | "body" | "auth" | "variable";

export interface SearchHit {
  path: string;
  name: string;
  method: string;
  field: SearchField;
  snippet: string;
  /** `[start, end)` offsets into `snippet` (JavaScript string indexes). */
  ranges: [number, number][];
}

// ---- plugins (rt_plugin::api, rt_app::plugins; wave 5) ----

export interface Permissions {
  /** Hosts for the `http` host call: exact, `*.example.com`, `{request}` or `*`. */
  network: string[];
  storage: boolean;
  variables: boolean;
}

/** `collection`: a panel next to the request tree, `request_path` is "" (wave 6). */
export type TabPlace = "request" | "response" | "collection";

export interface TabContribution {
  id: string;
  title: string;
  place: TabPlace;
  content_types: string[];
}

export interface ImporterContribution {
  id: string;
  label: string;
  extensions: string[];
}

export interface Contributions {
  prepare: boolean;
  response: boolean;
  tabs: TabContribution[];
  importers: ImporterContribution[];
  /** First-start choice of a bundled plugin (wave 6). */
  welcome: WelcomeChoice | null;
}

export interface WelcomeChoice {
  label: string;
  description: string;
}

export interface Manifest {
  id: string;
  name: string;
  version: string;
  api: number;
  description: string;
  license: string | null;
  homepage: string | null;
  permissions: Permissions;
  contributes: Contributions;
}

export interface PluginInfo {
  manifest: Manifest;
  granted: boolean;
  enabled: boolean;
}

export interface TabTarget {
  request_path: string;
  response_id: number | null;
}

export interface TreeItem {
  label: string;
  detail: string | null;
  children: TreeItem[];
  action: string | null;
}

export type FieldKind = "text" | "select" | "checkbox";

export interface FormField {
  name: string;
  label: string;
  kind: FieldKind;
  value: string;
  options: string[];
}

export interface ActionButton {
  id: string;
  label: string;
}

export type Block =
  | { type: "heading"; text: string }
  | { type: "text"; text: string }
  | { type: "notice"; level: NoteLevel; text: string }
  | { type: "key-value"; rows: [string, string][] }
  | { type: "table"; columns: string[]; rows: string[][] }
  | { type: "code"; language: string | null; text: string }
  | { type: "tree"; nodes: TreeItem[] }
  | { type: "form"; id: string; fields: FormField[]; submit: string }
  | { type: "actions"; actions: ActionButton[] };

export interface View {
  blocks: Block[];
}

export interface ViewReply {
  view: View;
  /** Opaque; passed back with the next call for the same tab. */
  state: unknown;
  /** Replaces the request in the editor (unsaved). */
  update_request: RequestSpec | null;
  /** New requests for the collection (unsaved; wave 6). */
  create_requests: NewRequest[];
  /** New environments, non-secret values only (unsaved; wave 6). */
  add_environments: PluginEnvironment[];
}

export interface NewRequest {
  folder: string;
  name: string;
  spec: RequestSpec;
}

export interface PluginEnvironment {
  name: string;
  variables: PluginVariable[];
}

export interface PluginVariable {
  name: string;
  value: string;
  secret: boolean;
}

/** The first-start question (wave 6): bundled plugins that offer a `welcome` choice. */
export interface WelcomeState {
  /** The user has answered (or skipped) the question already. */
  done: boolean;
  /** Bundled plugins with their choice. */
  choices: { plugin: string; choice: WelcomeChoice }[];
}

// ---- auth, cookies, secret scan (rt_app; wave 5) ----

export interface TokenInfo {
  key: string;
  label: string;
  obtained_at: number;
  expires_at: number | null;
  refreshable: boolean;
}

export interface DeviceCodePrompt {
  user_code: string;
  verification_uri: string;
  verification_uri_complete: string | null;
  expires_in: number;
}

export type AuthPrompt =
  | { kind: "browser"; url: string }
  | ({ kind: "device-code" } & DeviceCodePrompt)
  | { kind: "done" };

export interface CookieInfo {
  name: string;
  value: string;
  domain: string;
  path: string;
  expires: number | null;
  secure: boolean;
  http_only: boolean;
  same_site: string | null;
}

export interface SecretFinding {
  path: string;
  line: number;
  kind: string;
  snippet: string;
  hint: string;
}
