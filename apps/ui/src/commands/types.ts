// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// TypeScript mirror of the serde JSON of rt_core / rt_app types. Kept in sync by the contract
// test: `cargo test -p rt-app --test contract` writes `contract.generated.ts`, which annotates
// real serialized values with these types, so `npm run check` fails on any drift.
// Changing these types is an interface change (working rule 1).

import type {
  ChangeTarget,
  CollectionDiff,
  DeviceCodePrompt,
  DiffValue,
  FieldChange,
  FieldPath,
  GitCommit,
  GitFileCommit,
  RepositoryEntry,
  SecretFinding,
} from "@tailor-kit/git-ui/types";

// The Git and review types live in `packages/git-ui` (step K3); every import of this file keeps working.
export type * from "@tailor-kit/git-ui/types";

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
  | { type: "file"; path: string; content_type: string | null }
  /** Wave 8: sent as JSON `{query, variables}`; `variables` is JSON text (may be empty). */
  | { type: "graphql"; query: string; variables: string };

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
  /** Wave 7. */
  scripts: Script[];
  assertions: Assertion[];
  actions: VarAction[];
  /** Wave 8: omitted for HTTP. gRPC and WebSocket keep url, headers (metadata / handshake),
   *  auth, vars and scripts; `method` is `GRPC` or `WS`. */
  protocol?: Protocol;
}

// ---- scripts, assertions, actions (rt_core::model::script, rt_core::script; wave 7) ----

export type ScriptPhase = "before-request" | "after-response" | "tests";

export interface Script {
  phase: ScriptPhase;
  code: string;
}

/** Declarative check, e.g. `res.status eq 200`. Operators as in Bruno (see Rust docs). */
export interface Assertion {
  expression: string;
  operator: string;
  value: string;
  description: string | null;
  enabled: boolean;
}

export type VarScope = "runtime" | "environment" | "collection" | "global";

/** "Set variable from the response" (OpenCollection `set-variable`). */
export interface VarAction {
  phase: ScriptPhase;
  /** e.g. `res.body.token` (method `jsonq`) or a header name (method `header`). */
  expression: string;
  method: string;
  variable: string;
  scope: VarScope;
  enabled: boolean;
}

/** A variable set (`value` string) or unset (`null`) by a script or action. */
export interface VarChange {
  name: string;
  value: string | null;
  scope: VarScope;
}

export interface TestResult {
  name: string;
  passed: boolean;
  error?: string;
  /** `collection`, `folder:<path>` or `request`. */
  origin: string;
}

export type LogLevel = "debug" | "log" | "info" | "warn" | "error";

export interface ScriptLog {
  level: LogLevel;
  text: string;
  origin: string;
}

export type Flow = { kind: "continue" } | { kind: "skip" } | { kind: "next"; request: string | null };

export interface AssertionResult {
  expression: string;
  operator: string;
  expected: string;
  actual: string | null;
  passed: boolean;
  error?: string;
}

// ---- runner (rt_core::runner; wave 7) ----

/** One data row (CSV columns as strings, JSON values as they are). */
export type DataRow = [string, unknown][];

export interface RunPlan {
  requests: string[];
  data: DataRow[];
  iterations: number;
  delay_ms: number;
  bail: boolean;
  /** Wave 18: waiting for rate limits; omitted when off. */
  throttle?: Throttle;
}

export interface RunStep {
  iteration: number;
  request_path: string;
  request_name: string;
  report: SendReport;
  passed: boolean;
  skipped: boolean;
}

export interface RunSummary {
  requests: number;
  passed: number;
  failed: number;
  skipped: number;
  tests_passed: number;
  tests_failed: number;
  assertions_passed: number;
  assertions_failed: number;
  duration_ms: number;
  stopped: boolean;
}

export type RunEvent =
  | { kind: "started"; total: number }
  | { kind: "request"; iteration: number; request_path: string }
  | { kind: "step"; step: RunStep }
  /** Wave 18: waiting for a rate limit until `until` (unix ms). */
  | { kind: "waiting"; request_path: string; until: number; reason: string }
  | { kind: "finished"; summary: RunSummary };

/** Whether the open collection has scripts and whether the user trusts it to run them. */
export interface ScriptTrust {
  has_scripts: boolean;
  trusted: boolean;
}

// ---- variables (rt_core::vars) ----

export type VarSource =
  | { layer: "global" }
  | { layer: "collection" }
  | { layer: "environment"; name: string }
  | { layer: "folder"; path: string }
  | { layer: "request" }
  | { layer: "runtime" };

/** Wave 14 (variable completion, `rt_app::variables`): one variable a request, folder or the
 *  collection sees. Never carries a secret value. */
export interface ScopeVar {
  name: string;
  /** As defined (not resolved), secret values masked; `null` for secrets and value-less
   *  variables. */
  value: string | null;
  source: VarSource;
  secret: boolean;
  /** The secret manager of an external secret of the environment. */
  external: SecretProvider | null;
  /** A higher layer (or a later definition in the same layer) defines the same name. */
  overridden: boolean;
}

export interface VariableScope {
  /** Lowest precedence first (global, collection, environment, folders outer to inner,
   *  request, runtime); within a layer in file order. */
  variables: ScopeVar[];
  /** `{{names}}` used somewhere in the collection that no layer here defines (sorted). */
  undefined: string[];
}

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
  | { kind: "script"; message: string }
  /** Wave 10: the distribution's send guard refused the request; nothing was sent. */
  | { kind: "blocked"; message: { rule: string; reason: string } }
  | { kind: "transport"; message: TransportError };

// ---- send guard (rt_core::guard; wave 10) ----

export type Caller = "app" | "cli" | "agent";

export type SendPurpose = "main" | "script" | "egress" | "audit";

/** What a distribution's send guard is asked before a request goes out. */
export interface SendCheck {
  caller: Caller;
  purpose: SendPurpose;
  collection: string;
  request_path: string;
  environment: string | null;
  method: string;
  /** Resolved, secret values masked. */
  url: string;
  effect: Effect;
  /** The collection's agent policy marks the environment as production. */
  production: boolean;
}

export type GuardVerdict =
  | { decision: "allow" }
  | { decision: "refuse"; rule: string; reason: string };

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
  /** Wave 7, each omitted when empty. */
  tests?: TestResult[];
  assertions?: AssertionResult[];
  logs?: ScriptLog[];
  var_changes?: VarChange[];
  flow?: Flow;
  /** Wave 18, each omitted when empty. */
  segments?: TimingSegment[];
  rate_limit?: RateLimits;
  checks?: Observation[];
  /** Wave 19: the trace of the send (`traceparent` the app set, or the id the response
   *  carried); only when there is one. */
  trace?: TraceRef;
}

/** Wave 19 (`rt_core::trace`). Never carries a secret. */
export interface TraceRef {
  trace_id: string;
  span_id?: string;
  /** The app put the `traceparent` on the request. */
  injected: boolean;
}

// ---- app (rt_app) ----

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
  /** A Git host's review API failed or refused (wave 12, strand E). */
  | { kind: "review"; message: string }
  | { kind: "unsupported"; message: string }
  | { kind: "shell"; message: string };

export interface StoredResponse {
  /** `report.response.body` is a preview of at most 64 KiB; read the rest by lines. */
  report: SendReport;
  response_id: number | null;
  size: number;
  line_count: number;
  is_text: boolean;
  /** `false`: the collection switched exchange observers off (`extensions.requesttailor.observe`);
   *  `AppExtension.onExchange` is not called. Absent: on. */
  observe?: boolean;
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

// ---- import into the open collection (rt_app::import, wave 13) ----

export interface ImportedFile {
  path: string;
  text: string;
}

/** A collection converted by an extension's importer, for `ImportApi.adopt`. */
export interface ExternalImport {
  collection_name: string;
  files: ImportedFile[];
  report: LossReport;
}

/** What `ImportApi.writeInto` added to the open collection (unsaved). */
export interface FolderImport {
  folder: string;
  name: string;
  requests: number;
  folders: number;
  environments_added: string[];
  environments_extended: string[];
  /** Other files of the import, staged below the folder. */
  files: string[];
  /** Collection-level settings the requests carry instead (in addition to the plan's report). */
  losses: Loss[];
  /** The kept OpenAPI spec, now named in the collection's `contract.spec`; `null` if none was
   *  kept or the collection already names one. Absent from older shells. */
  contract_spec?: string | null;
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
  /** Wave 18: paging rules for responses of other services; omitted when empty. */
  paging?: PagingContribution[];
  /** Wave 19: rules that say how writing requests are put back; omitted when empty. */
  compensations?: CompensationContribution[];
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

// ---- protocols and sessions (rt_core::model::protocol, rt_core::stream; wave 8) ----

export type MessageKind = "text" | "json" | "xml" | "binary";

export interface MessageVariant {
  title: string;
  selected: boolean;
  kind: MessageKind;
  /** Binary: base64. */
  data: string;
}

export type GrpcMethodType = "unary" | "client-streaming" | "server-streaming" | "bidi-streaming";

export interface GrpcSpec {
  method: string;
  method_type: GrpcMethodType;
  proto_file: string | null;
  messages: MessageVariant[];
}

export interface WebSocketSpec {
  messages: MessageVariant[];
  timeout_ms: number | null;
  keep_alive_ms: number | null;
}

export type Protocol =
  | { type: "http" }
  | ({ type: "grpc" } & GrpcSpec)
  | ({ type: "websocket" } & WebSocketSpec);

export type Frame = { type: "text"; data: string } | { type: "binary"; data: string };

export type Direction = "sent" | "received";

export interface StreamHead {
  status: number;
  version: HttpVersion;
  headers: [string, string][];
  url: string;
}

export type StreamEvent =
  | { event: "opened"; at_ms: number; head: StreamHead }
  | {
      event: "message";
      at_ms: number;
      direction: Direction;
      frame: Frame;
      /** SSE `event:`, gRPC message type. */
      name?: string;
      /** SSE `id:`. */
      id?: string;
    }
  | {
      event: "closed";
      at_ms: number;
      code: number | null;
      reason: string;
      trailers?: [string, string][];
      by_client: boolean;
    }
  | { event: "error"; at_ms: number; error: TransportError };

/** Masked history of one session (`rt send` output for session requests). */
export interface StreamLog {
  version: number;
  request_path: string;
  environment: string | null;
  kind: string;
  url: string;
  events: StreamEvent[];
  notes?: string[];
}

// ---- gRPC (rt_grpc; wave 8) ----

export interface GrpcMethod {
  name: string;
  path: string;
  method_type: GrpcMethodType;
  input_type: string;
  output_type: string;
}

export interface GrpcService {
  name: string;
  methods: GrpcMethod[];
}

export type GrpcError =
  | { kind: "proto"; message: string }
  | { kind: "reflection"; message: string }
  | { kind: "unknown-method"; message: string }
  | { kind: "message"; message: string };

// ---- agents (rt_core::model::agents, rt_app::agents; wave 8) ----

export type AgentAccess = "none" | "read" | "write";

export interface EnvironmentPolicy {
  environment: string;
  access: AgentAccess;
  production: boolean;
}

export interface AgentPolicy {
  access: AgentAccess;
  environments: EnvironmentPolicy[];
}

export type Effect = "read" | "write";

export type Verdict =
  | { decision: "allowed" }
  | { decision: "needs-approval" }
  | { decision: "refused"; reason: string };

/** `rt_app::agents::AgentCheck`: what the agent rules say about sending one request. */
export interface AgentCheck {
  effect: Effect;
  verdict: Verdict;
}

export interface PendingApproval {
  id: string;
  at: number;
  agent: string;
  collection: string;
  collection_name: string;
  request_path: string;
  environment: string | null;
  method: string;
  url: string;
  reason: string;
}

export type ApprovalDecision = "approved" | "denied";

export type AuditOutcome = "read" | "sent" | "refused" | "denied" | "timed-out" | "failed";

export interface AuditEntry {
  at: number;
  agent: string;
  tool: string;
  collection: string;
  request_path?: string;
  environment?: string;
  method?: string;
  url?: string;
  status?: number;
  outcome: AuditOutcome;
  approved_by?: string;
  /** Wave 10: the send guard's rule when it refused the call (outcome `refused`). */
  rule?: string;
  detail?: string;
}

// ---- AI assistant (rt_app::ai; wave 8, extended in wave 16) ----

export type AiNameTarget = "request" | "folder" | "variable";

export type AiTask =
  | { task: "create-request"; description: string; folder: string }
  | { task: "explain-error"; path: string; response: number }
  | { task: "suggest-tests"; path: string; response: number }
  // Wave 16:
  | { task: "request-from-snippet"; snippet: string; folder: string }
  | { task: "explain-response"; path: string; response: number }
  | { task: "suggest-assertions"; path: string; response: number }
  | { task: "suggest-names"; target: AiNameTarget; path: string }
  | { task: "draft-docs"; path: string };

export interface AiAnswer {
  text: string;
  request?: RequestSpec;
  scripts?: Script[];
  assertions?: Assertion[];
  /** Wave 16, `suggest-names`. */
  names?: string[];
  /** Wave 16, `draft-docs` (Markdown). */
  docs?: string;
}

/** Wave 16: what of the active collection the core puts into a call, masked. */
export interface AiScope {
  request?: string;
  response?: number;
  folder?: string;
}

// ---- MCP client (rt_app::mcp_client; wave 8) ----

export type McpServerConfig =
  | {
      transport: "stdio";
      command: string;
      args: string[];
      env: [string, string][];
      cwd: string | null;
    }
  | { transport: "http"; url: string; headers: [string, string][] };

export interface McpServerInfo {
  name: string;
  version: string;
  protocol_version: string;
  capabilities: string[];
  instructions: string | null;
}

export interface McpTool {
  name: string;
  title: string | null;
  description: string | null;
  input_schema: unknown;
  output_schema: unknown | null;
  annotations: unknown | null;
}

export type McpContent =
  | { type: "text"; text: string }
  | { type: "image"; mime_type: string; data: string }
  | { type: "audio"; mime_type: string; data: string }
  | { type: "resource"; uri: string; text: string | null }
  | { type: "other"; value: unknown };

export interface McpCallResult {
  content: McpContent[];
  structured: unknown | null;
  is_error: boolean;
  time_ms: number;
  /** Progress, log and other notifications sent while the call ran (wave 14); left out when none. */
  notifications?: McpNotification[];
}

/** A notification from the server, as sent (masked). */
export interface McpNotification {
  method: string;
  params: unknown;
}

/** An entry of `resources/list` (wave 14). */
export interface McpResource {
  uri: string;
  name: string;
  title: string | null;
  description: string | null;
  mime_type: string | null;
  size: number | null;
}

/** One item of `resources/read`: text, or binary data as base64 (`blob`). */
export interface McpResourceContents {
  uri: string;
  mime_type: string | null;
  text: string | null;
  blob: string | null;
}

export interface McpPromptArgument {
  name: string;
  title: string | null;
  description: string | null;
  required: boolean;
}

/** An entry of `prompts/list` (wave 14). */
export interface McpPrompt {
  name: string;
  title: string | null;
  description: string | null;
  arguments: McpPromptArgument[];
}

export interface McpPromptMessage {
  role: string;
  content: McpContent;
}

/** What `prompts/get` returned. */
export interface McpPromptResult {
  description: string | null;
  messages: McpPromptMessage[];
}

/** `rt mcp` on the open collection as a server to connect to (rt_app::mcp_client::McpSelfServer). */
export interface McpSelfServer {
  /** `null` where it cannot run (the web app); `hint` says why. */
  server: McpServerConfig | null;
  hint: string;
}

export interface McpTest {
  name: string;
  server: McpServerConfig;
  tool: string;
  arguments: unknown;
  assertions: Assertion[];
}

export interface McpTestResult {
  path: string;
  passed: boolean;
  result: McpCallResult | null;
  error: string | null;
  assertions: AssertionResult[];
}

// ---- Wave 12 (Git for APIs). Rust: rt_app::git (revisions), rt_format::{diff, merge,
// contract}, rt_app::{diffs, merges, compare, bisect, timeline, review, contract, rescue}. ----

export type Side = "ours" | "theirs";

export type ConflictChoice =
  | { kind: "field"; target: ChangeTarget; field: FieldPath; side: Side }
  | { kind: "file"; path: string; side: Side };

export interface RevisionRunRequest {
  rev: string;
  folder?: string | null;
  /** Only these requests (paths at `rev`), in this order. */
  paths?: string[] | null;
}

export interface RevisionRun {
  commit: GitCommit;
  steps: RunStep[];
  summary: RunSummary;
}

export interface CompareRequest {
  from: string;
  to: string;
  folder?: string | null;
  paths?: string[] | null;
}

export interface ValueChange<T> {
  old: T | null;
  new: T | null;
}

export interface HeaderChange {
  name: string;
  old: string | null;
  new: string | null;
}

export interface BodyChange {
  /** JSON Pointer, e.g. `/items/0/id`. */
  pointer: string;
  old: unknown | null;
  new: unknown | null;
}

export interface TestChange {
  name: string;
  old: boolean | null;
  new: boolean | null;
}

export interface RequestComparison {
  path: string;
  name: string;
  same: boolean;
  status: ValueChange<number> | null;
  headers: HeaderChange[];
  body: BodyChange[];
  body_text: boolean;
  tests: TestChange[];
  error: ValueChange<string> | null;
  /** Wave 14 (strand H): both responses for the side-by-side view. */
  sides?: ResponseSides;
}

/** `rt_app::compare::ResponseSides`; a side is `null` where the request was not sent. */
export interface ResponseSides {
  old: ResponseSnapshot | null;
  new: ResponseSnapshot | null;
}

/** One response as the comparison shows it, masked. */
export interface ResponseSnapshot {
  status: number;
  duration_ms: number;
  size: number;
  headers: [string, string][];
  content_type?: string;
  /** Text preview (at most 256 KiB); absent for binary bodies. */
  body?: string;
  truncated: boolean;
}

export interface RunComparison {
  from: GitCommit;
  to: GitCommit;
  requests: RequestComparison[];
  from_summary: RunSummary;
  to_summary: RunSummary;
}

export interface BisectRequest {
  good: string;
  bad: string;
  folder?: string | null;
}

export type BisectVerdict = "good" | "bad" | "skip";

export interface BisectStep {
  commit: GitCommit;
  verdict: BisectVerdict;
  failed: string[];
  remaining: number;
}

export interface BisectOutcome {
  first_bad: GitCommit | null;
  candidates: number;
  steps: BisectStep[];
}

export type RecordSource = "runner" | "compare" | "bisect";

export interface RecordedTest {
  name: string;
  passed: boolean;
}

/** Results of one request in one run, recorded locally under a commit. */
export interface TestRecord {
  collection: string;
  request: string;
  environment: string | null;
  time: number;
  source: RecordSource;
  passed: boolean;
  status: number | null;
  tests: RecordedTest[];
  /** Wave 18: the send's duration; omitted when not recorded. */
  duration_ms?: number;
}

export interface TimelineEntry {
  commit: GitFileCommit;
  fields: FieldChange[];
  tests: TestRecord[];
}

export interface RequestTimeline {
  path: string;
  entries: TimelineEntry[];
}

export interface FieldBlame {
  field: FieldPath;
  commit: GitCommit;
  value: DiffValue | null;
}

export interface ContractSpec {
  path: string;
  title: string;
  version: string;
  configured: boolean;
}

export type FindingKind = "no-operation" | "unknown-parameter" | "missing-parameter" | "body-mismatch";

export interface ContractFinding {
  request: string;
  kind: FindingKind;
  operation: string | null;
  message: string;
}

export interface ContractReport {
  findings: ContractFinding[];
  uncovered: string[];
  operations: number;
  covered: number;
}

export type SpecChangeKind =
  | "operation-added"
  | "operation-removed"
  | "parameter-added"
  | "parameter-removed"
  | "required-added"
  | "field-removed"
  | "field-added"
  | "type-changed"
  | "response-changed"
  | "type-removed"
  | "enum-value-removed"
  | "nullability-changed"
  | "number-changed"
  | "streaming-changed"
  | "other";

export interface SpecChange {
  kind: SpecChangeKind;
  breaking: boolean;
  operation: string | null;
  location: string;
  message: string;
  requests: string[];
}

export interface SpecChanges {
  from_version: string | null;
  to_version: string | null;
  changes: SpecChange[];
}

export interface HistoryFinding {
  /** `<commit oid>:<path>:<line>` of the first occurrence (`.2`, `.3`, … after the line for a
   *  second, third value on the same line). */
  id: string;
  finding: SecretFinding;
  first: GitCommit;
  commits: string[];
  branches: string[];
  in_head: boolean;
}

export interface ScanProgress {
  scanned: number;
  total: number;
}

export interface HistoryScan {
  commits: number;
  findings: HistoryFinding[];
}

export interface RescuePlan {
  findings: string[];
  variable: string;
  environment: string;
}

export interface RewrittenBranch {
  name: string;
  old: string;
  new: string;
  /** Has an upstream: needs a force push to remove the value there. */
  pushed: boolean;
  /** The upstream's remote (`origin`), for the force-push command. */
  remote: string | null;
}

export interface RescueResult {
  rewritten: number;
  branches: RewrittenBranch[];
  /** Ref prefix of this rescue's backups, `refs/requesttailor/backup/<unix>/`. */
  backup: string;
  /** Tags that point at a rewritten commit (tags are not rewritten: they keep the value). */
  tags: string[];
}

// ---- collections in the sidebar (rt_app::collections; wave 14, strand A) ----

/** One collection in the sidebar. */
export interface CollectionEntry {
  /** `CollectionInfo.id`: stable on this device; also the key of secrets, tokens, cookies. */
  id: string;
  name: string;
  /** For display only. */
  location: string;
  /** The root (`CollectionRoot.id`) it was found in. */
  root: string;
  /** Its folder relative to the root, `""` if the root is the collection. */
  dir: string;
  /** The Git repository that holds it; collections with the same `repo` share the Git panel. */
  repo: string | null;
  /** Number of unsaved changes. */
  dirty: number;
}

/** A folder the user opened or added ("Add collection…"), remembered per device. */
export interface CollectionRoot {
  id: string;
  name: string;
  location: string;
  /** Ids of its collections, in sidebar order; wave 15: empty for a repository root (a Git
   *  repository without any collection). */
  collections: string[];
  /** More than `MAX_COLLECTIONS` (64) were found; the rest is not shown. */
  truncated: boolean;
}

/** What the sidebar shows; no file lists it (files stay the truth). */
export interface CollectionSet {
  roots: CollectionRoot[];
  /** All collections in sidebar order. */
  collections: CollectionEntry[];
  /** The active collection: the one every per-collection command acts on. */
  active: string | null;
  /** Wave 15: every repository of the set (those holding a collection, then repository roots
   *  without one), in sidebar order. */
  repositories: RepositoryEntry[];
  /** Wave 15: the repository every `GitApi` command acts on (chosen last, else the active
   *  collection's). */
  active_repository?: string;
}

/** A request in one of the set's collections (tabs, search hits, history, runner targets). */
export interface RequestRef {
  collection: string;
  path: string;
}

export interface CollectionSearchHit extends SearchHit {
  collection: string;
}

/** What saving wrote, per collection. */
export interface CollectionChanges {
  collection: string;
  changes: FileChange[];
}

/** A new collection from the "New…" dialog. */
export interface NewCollection {
  /** `name` in `opencollection.yml`. */
  name: string;
  /** Root to create it in; `null`: the shell asks for a folder and adds it as a new root. */
  root: string | null;
  /** Folder inside the root (relative, `/`-separated); must not exist yet or be empty. */
  dir: string;
  /** `baseUrl` of the first environment, else of the collection. */
  base_url?: string | null;
  environment?: string | null;
}

// ---- Wave 14: Git for any repository (`rt_app::git::{graph, actions}`) ----
// Paths are repository-relative with a leading `/`. Nothing assumes the repository is a
// collection: a file with `collection` set gets the semantic diff, every other the line diff.

// ---- OpenAPI import wizard (rt_format::import::openapi_types, rt_app::openapi) ----

export interface SpecOverview {
  title: string;
  version: string;
  description: string | null;
  /** `3.0.3`, `3.1.0`, `2.0` (Swagger). */
  spec_version: string;
  main_file: string;
  files: number;
  servers: SpecServer[];
  tags: SpecTag[];
  operations: SpecOperation[];
  security: SpecSecurityScheme[];
  losses: Loss[];
}

export interface SpecServer {
  url: string;
  description: string | null;
  suggested_name: string;
  variables: SpecServerVariable[];
}

export interface SpecServerVariable {
  name: string;
  default: string;
  values: string[];
  description: string | null;
}

export interface SpecTag {
  name: string;
  description: string | null;
  operations: number;
}

export interface SpecOperation {
  /** `GET /pets/{petId}`. */
  key: string;
  method: string;
  path: string;
  tags: string[];
  summary: string | null;
  operation_id: string | null;
  deprecated: boolean;
}

export interface SpecSecurityScheme {
  name: string;
  /** `basic`, `bearer`, `digest`, `api-key`, `oauth2`, `openid-connect`, `mutual-tls`, `other`. */
  kind: string;
  detail: string;
  /** What the requests get, e.g. `Bearer token {{token}}`; `null`: not imported. */
  maps_to: string | null;
  variables: string[];
}

export type OpenApiGrouping = "tag" | "path" | "flat";
export type OpenApiNaming = "summary" | "operation-id";
export type OpenApiExamples = "spec" | "schema";

export interface OpenApiServerChoice {
  index: number;
  /** `null`: no environment for this server. */
  environment: string | null;
}

export interface OpenApiOptions {
  /** `null`: all operations. */
  operations: string[] | null;
  grouping: OpenApiGrouping;
  naming: OpenApiNaming;
  examples: OpenApiExamples;
  /** `null`: every server, named as suggested. */
  servers: OpenApiServerChoice[] | null;
  default_server: number | null;
  collection_name: string | null;
  keep_spec: boolean;
}

export interface FetchedSpec {
  url: string;
  files: FetchedFile[];
  losses: Loss[];
}

export interface FetchedFile {
  name: string;
  text: string;
}

// ---- run wizard and response check (rt_format::contract, rt_app::contract) ----

export type InputPlace = "variable" | "path" | "query" | "header" | "body";
export type ProblemKind = "missing" | "invalid";

export interface InputProblem {
  kind: ProblemKind;
  message: string;
  details?: string[];
}

export interface InputField {
  /** `variable:baseUrl`, `path:petId`, `query:limit`, `header:X-Id`, `body`. */
  id: string;
  place: InputPlace;
  name: string;
  required: boolean;
  secret: boolean;
  auth: boolean;
  value: string | null;
  problem: InputProblem | null;
  /** JSON Schema, local `$ref`s inlined. */
  schema: unknown;
  description: string | null;
  example: string | null;
}

export interface ServerOption {
  url: string;
  description: string | null;
}

export interface SendRequirements {
  operation: string;
  summary: string | null;
  deprecated: boolean;
  spec: string;
  ready: boolean;
  fields: InputField[];
  servers: ServerOption[];
}

export type DifferencePlace = "status" | "header" | "content-type" | "body";

export interface ResponseDifference {
  place: DifferencePlace;
  path: string;
  expected: string;
  got: string;
}

export interface ResponseCheck {
  operation: string;
  status: number;
  matched: string | null;
  differences: ResponseDifference[];
  body_skipped: string | null;
}

// ---- Wave 16: changelog facts (rt_app::contract::{ChangelogFacts, ChangelogCommit}) ----

export interface ChangelogCommit {
  oid: string;
  summary: string;
  body?: string;
  author: string;
  /** A pull or merge request number found in the message. */
  pull?: number;
}

/** Everything deterministic about a range; a writer of release notes states nothing else. */
export interface ChangelogFacts {
  from: string;
  to: string;
  title: string;
  spec: SpecChanges;
  collection: CollectionDiff;
  sections?: string[];
  commits?: ChangelogCommit[];
  /** The changelog as `rt changelog` writes it. */
  markdown: string;
}

// ---- Wave 15: Git client for any repository (`rt_app::git::{repos, files, history, plumbing,
// index, signing}`, `rt_app::review::NewPullRequest`). Paths repository-relative with a leading
// `/`; the default commit stays "without staging". ----

// ---- Wave 17: snapshot tests (rt_app::snapshots) ----

/** A JSON body as is, any other text body as a JSON string, or hash and length when it is
 *  too large. */
export type SnapshotBody =
  | { $digest: string; $length: number }
  | string
  | number
  | boolean
  | null
  | unknown[]
  | { [key: string]: unknown };

/** A frozen response, the content of a `<request>.snap.json` file. */
export interface Snapshot {
  version: number;
  status: number;
  /** Allowlisted headers, lowercase names. */
  headers: Record<string, string>;
  body: SnapshotBody;
  /** Paths the comparison skips. */
  ignore: string[];
}

export type SnapshotDiffKind = "changed" | "added" | "removed" | "type";

export interface SnapshotDiff {
  /** Where, as a path of the expression language (`status`, `body.items[0].id`). */
  path: string;
  kind: SnapshotDiffKind;
  expected: unknown | null;
  actual: unknown | null;
}

export type SnapshotState = "matches" | "differs" | "missing" | "none";

export interface SnapshotCheck {
  request_path: string;
  /** The companion file, relative to the collection; empty for `none`. */
  snapshot: string;
  state: SnapshotState;
  differences: SnapshotDiff[];
}

export interface SnapshotFreeze {
  request_path: string;
  snapshot: string;
  /** The request got its assertion with this freeze. */
  assertion_added: boolean;
  content: Snapshot;
}

export interface FreezeOptions {
  /** Keep the `etag` header (off by default). */
  keep_etag?: boolean;
  /** The assertion's `value` for a new snapshot; absent: `<request name>.snap.json`. */
  file?: string | null;
}

export interface SnapshotEntry {
  request_path: string;
  snapshot: string;
  /** The file exists (staged or saved). */
  exists: boolean;
}

// ---- Wave 17: sharing and copying (rt_app::share) ----

export interface ShareVariable {
  name: string;
  /** Only for non-secret variables and only with `include_values`. */
  value: string | null;
  secret: boolean;
}

/** What a link carries: the definition with `{{variables}}` unresolved and no secret value. */
export interface SharePayload {
  version: number;
  name: string;
  request: RequestSpec;
  variables: ShareVariable[];
}

export interface ShareOptions {
  /** "Include variable values": non-secret values of `environment` (default off). */
  include_values?: boolean;
  environment?: string | null;
  /** The link's base; absent or empty: only the payload as `rt:<payload>`. */
  base?: string | null;
}

export interface ShareLink {
  /** `<base>#rt=<payload>`, or `rt:<payload>` without a base. */
  text: string;
  /** Exactly what the link carries, as text ("What is in this link"). */
  preview: string;
  payload: SharePayload;
  length: number;
  fits_qr: boolean;
}

export type CopyKind = "curl" | "fetch" | "python";

export interface CopyOptions {
  /** Put the values of non-secret variables into the snippet (default off). */
  resolve_variables?: boolean;
  environment?: string | null;
}

export interface CopiedAs {
  kind: CopyKind;
  text: string;
  /** What the snippet leaves out. */
  notes: string[];
}

// ---- wave 18: checking and watching ----

/** A named part of the time (`rt_core::segments`), e.g. a plugin's database time inside the
 *  server's wait. */
export interface TimingSegment {
  id: string;
  label: string;
  ms: number;
  /** `"server"` puts it inside the server wait; since wave 19 also the `id` of another
   *  segment of the same report (a tree). */
  parent?: string;
  /** Wave 19: milliseconds after the server began handling the request (its own clock);
   *  `undefined`: unknown. */
  offset_ms?: number;
}

export interface RateWindow {
  limit: number;
  seconds: number;
}

/** Rate limits a response announced (`rt_core::ratelimit`). */
export interface RateLimits {
  limit?: number;
  remaining?: number;
  reset_ms?: number;
  retry_after_ms?: number;
  windows?: RateWindow[];
}

export type ThrottleMode = "off" | "respect";

export interface Throttle {
  mode?: ThrottleMode;
  max_wait_ms?: number;
  retries?: number;
}

export type Severity = "info" | "warn";

/** One passive observation about a request or response (`rt_core::inspect`). */
export interface Observation {
  id: string;
  severity: Severity;
  location: string;
  why: string;
  fix: string;
}

export type CursorPlace = "query" | "body" | "header";

export interface CursorParam {
  place: CursorPlace;
  name: string;
}

export type PagingRule =
  | { kind: "link-header" }
  | { kind: "json-url"; next: string }
  | { kind: "cursor"; next: string; param: CursorParam };

export interface PagingSpec {
  rule: PagingRule;
  merge?: string;
}

export interface PagingMatch {
  hosts?: string[];
  content_types?: string[];
  url_pattern?: string;
}

export interface PagingContribution {
  id: string;
  match: PagingMatch;
  rule: PagingSpec;
}

export interface PagingLimits {
  max_pages: number;
  max_bytes: number;
  max_seconds: number;
}

export type PagingOrigin = { kind: "request" } | { kind: "collection" } | { kind: "plugin"; plugin: string; id: string };

export interface EffectivePaging {
  spec: PagingSpec;
  origin: PagingOrigin;
}

export type PagingMode = "next" | "all";
export type PagingRuleScope = "request" | "collection";

export interface PagingRun {
  path: string;
  /** As `send` takes it; the shell adds the secret values (never the webview). */
  input: { environment: string | null; vars: [string, string][]; secrets?: [string, string][] };
  mode: PagingMode;
  response_id?: number | null;
  limits?: PagingLimits | null;
}

export type PagingStop =
  | { kind: "done" }
  | { kind: "max-pages" }
  | { kind: "max-bytes" }
  | { kind: "max-seconds" }
  | { kind: "loop" }
  | { kind: "other-origin"; message: string }
  | { kind: "error-status"; message: number }
  | { kind: "refused"; message: string }
  | { kind: "not-read" };

export interface PagedResult {
  response: StoredResponse;
  pages: number;
  stopped: PagingStop;
  more: boolean;
}

export interface EnvCompareRequest {
  environments: string[];
  folder?: string | null;
  paths?: string[] | null;
  ignore_env_values?: boolean;
  production_confirmed?: boolean;
}

export interface SkippedRequest {
  path: string;
  name: string;
  reason: string;
}

export interface EnvPair {
  environment: string;
  requests: RequestComparison[];
  reference_summary: RunSummary;
  summary: RunSummary;
}

export interface EnvComparison {
  reference: string;
  pairs: EnvPair[];
  skipped: SkippedRequest[];
}

export type ClaimKind = "text" | "time" | "list" | "other";

export interface JwtClaim {
  name: string;
  value: unknown;
  kind: ClaimKind;
  meaning: string | null;
}

export type JwtWarningKind = "expired" | "not-yet-valid" | "expires-soon" | "algorithm-none";

export interface JwtWarning {
  kind: JwtWarningKind;
  text: string;
}

export interface JwtInfo {
  header: unknown;
  algorithm: string | null;
  claims: JwtClaim[];
  warnings: JwtWarning[];
  expires_at: number | null;
  signature_verified: boolean;
}

export type TokenLocation =
  | { kind: "authorization" }
  | { kind: "auth" }
  | { kind: "query" }
  | { kind: "body"; path: string }
  | { kind: "cookie"; name: string };

export interface FoundToken {
  location: TokenLocation;
  token: JwtInfo;
}

export type CheckScope = "device" | "collection";

export interface InspectTarget {
  path: string;
  environment?: string | null;
  response_id?: number | null;
}

export interface InboxSettings {
  port: number;
  status: number;
  body: string;
  content_type?: string;
  delay_ms: number;
}

export interface CapturedRequest {
  id: number;
  at: number;
  method: string;
  target: string;
  headers: [string, string][];
  body: string | null;
  size: number;
}

export type InboxEvent =
  | { kind: "started"; port: number }
  | { kind: "stopped" }
  | { kind: "request"; request: CapturedRequest }
  | { kind: "dropped"; count: number }
  | { kind: "cleared" };

export interface InboxStatus {
  running: boolean;
  port?: number;
  url?: string;
  held: number;
  dropped: number;
}

export interface InboxPage {
  events: InboxEvent[];
  next: number;
}

/** A captured request as a request of the collection, unsaved (`InboxApi.makeRequest`). */
export interface InboxDraft {
  name: string;
  spec: RequestSpec;
  secrets: string[];
}

export type LintRule = "no-tests" | "hard-coded-host" | "orphan-variable" | "duplicate-name";
export type LintLevel = "off" | "info" | "warn" | "error";

export interface LintSettings {
  rules: Record<string, LintLevel>;
  ignore: string[];
  min_score?: number | null;
}

export interface LintFinding {
  rule: LintRule;
  level: LintLevel;
  path: string;
  message: string;
  location?: string | null;
}

export interface RuleScore {
  rule: LintRule;
  ok: number;
  total: number;
}

export interface HealthScore {
  score: number;
  rules: RuleScore[];
}

export interface FolderScore {
  folder: string;
  score: HealthScore;
}

export interface LintReport {
  collection: string;
  score: HealthScore;
  folders: FolderScore[];
  findings: LintFinding[];
  ignored: number;
}

export type ViewLanguage = "jsonpath" | "jq";
export type ViewDisplay = "json" | "table" | "text";

export interface ResponseView {
  name: string;
  language: ViewLanguage;
  expression: string;
  display: ViewDisplay;
  columns?: string[];
}

export interface ViewResult {
  display: ViewDisplay;
  values: unknown[];
  columns?: string[];
  rows?: unknown[][];
  notice?: string | null;
}

export type SchemaKind = "graphql" | "protobuf";

export interface CatalogueWasm {
  url: string;
  sha256: string;
  size: number;
}

export interface CatalogueEntry {
  id: string;
  name: string;
  version: string;
  api: number;
  description?: string;
  license?: string | null;
  homepage?: string | null;
  wasm: CatalogueWasm;
  permissions?: Permissions;
}

export interface CatalogueIndex {
  version: number;
  name: string;
  plugins: CatalogueEntry[];
}

export interface CatalogueSource {
  url: string;
  commit?: string | null;
  name?: string | null;
}

export interface SourceIndex {
  source: CatalogueSource;
  index: CatalogueIndex;
}

export interface InstallPlan {
  source: string;
  entry: CatalogueEntry;
  /** Where the module was downloaded from. */
  wasm_url: string;
  sha256: string;
  permissions: Permissions;
  permissions_differ: boolean;
  update: boolean;
  more_permissions: boolean;
  warnings: string[];
}

export interface InstalledSource {
  plugin: string;
  source: string;
  version: string;
  sha256: string;
}

export interface CatalogueUpdate {
  plugin: string;
  source: string;
  installed: string;
  available: string;
}

// ---- wave 19 ----

// Egress for extensions (`rt_app::egress`, strand B).
export interface EgressRequest {
  method: "GET" | "POST";
  url: string;
  headers?: [string, string][];
  body?: string;
  extension: string;
  purpose: string;
  collection?: string;
  /** The environment whose secret variable `{{secret:<name>}}` names (default: the first with a value). */
  environment?: string;
}

export interface EgressResponse {
  status: number;
  headers: [string, string][];
  body: string;
  truncated?: boolean;
  took_ms: number;
}

export interface EgressHost {
  host: string;
  port?: number;
  extension: string;
  purpose: string;
  allowed_at: number;
}

export type EgressError =
  | { kind: "not-allowed"; host: string; port: number | null }
  | { kind: "blocked"; rule: string; reason: string }
  | { kind: "limit"; message: string }
  | { kind: "transport"; message: string };

// The test-data journal (`rt_app::journal`, strand C).
export interface JournalSettings {
  enabled?: boolean;
  id_path?: string;
}

export interface JournalEntry {
  id: number;
  session: string;
  at: number;
  environment?: string;
  request_path: string;
  method: string;
  url: string;
  status: number;
  location?: string;
  etag?: string;
  resource_id?: string;
  reverted_at?: number;
}

export interface JournalQuery {
  limit?: number;
  before?: number;
  session?: string;
  only_open?: boolean;
}

export interface JournalPage {
  entries: JournalEntry[];
  total: number;
}

export type CompensationKind = "delete" | "restore" | "recreate" | "manual";

export interface Compensation {
  kind: CompensationKind;
  request: RequestSpec;
  state_hash?: string;
  probe?: RequestSpec;
  description: string;
}

export interface NotReversible {
  reason: string;
}

export type CompensationAnswer =
  | { kind: "compensation"; compensation: Compensation }
  | { kind: "not-reversible"; reason: string };

export interface CompensationItem {
  entry_id: number;
  answer: CompensationAnswer;
}

export interface ManualRevert {
  entry_id: number;
  request_path: string;
}

export type StepState = "planned" | "not-reversible";

export interface RevertStep {
  entry_id: number;
  state: StepState;
  method: string;
  url: string;
  effect: Effect;
  description: string;
  compensation?: Compensation;
  reason?: string;
}

export interface RevertPlan {
  environment: string | null;
  production: boolean;
  steps: RevertStep[];
  truncated?: boolean;
}

export interface RevertOptions {
  production_confirmed?: boolean;
}

export type StepResult = "reverted" | "skipped-conflict" | "skipped-not-reversible" | "failed";

export interface StepOutcome {
  entry_id: number;
  result: StepResult;
  message: string;
}

export interface RevertOutcome {
  steps: StepOutcome[];
}

export interface PluginCompensationRule {
  plugin: string;
  rule: CompensationContribution;
}

export interface PreRead {
  status: number;
  body: string | null;
  etag: string | null;
}

export interface EntryStates {
  entry_id: number;
  before: PreRead | null;
  after: PreRead | null;
}

/** `contributes.compensations` of a plugin manifest (wave 19). */
export interface CompensationContribution {
  id: string;
  match: {
    hosts?: string[];
    content_types?: string[];
    url_pattern?: string;
    methods?: string[];
  };
  rule: {
    id_from?: string[];
    resource_url?: string;
    etag_from?: string[];
    token?: { fetch_header: string; fetch_value: string; read_header: string; send_header: string };
    not_reversible?: { url_pattern: string; reason: string }[];
  };
}

// The two-user access check (`rt_app::access`, strand D).
export interface AccessProfile {
  a: string;
  b: string;
}

export interface AccessOptions {
  allow_writes?: boolean;
  b_must_be_denied?: boolean;
  rate?: number;
}

export type AccessVerdict =
  | { kind: "ok" }
  | { kind: "broken-object-access" }
  | { kind: "broken-function-access" }
  | { kind: "inconclusive" }
  | { kind: "skipped"; reason: string };

export interface AccessEvidence {
  profile: string;
  status: number | null;
  header_names: string[];
  values: [string, string][];
}

export interface AccessFinding {
  verdict: AccessVerdict;
  message: string;
  evidence: AccessEvidence[];
}

export interface AccessCheck {
  path: string;
  verdict: AccessVerdict;
  findings: AccessFinding[];
  status_a: number | null;
  status_b: number | null;
}

// Minimal failing request (`rt_app::shrink`, strand E).
export type ShrinkPredicate =
  | { kind: "status"; status: number }
  | { kind: "server-error" }
  | { kind: "assertion-fails"; assertion?: string }
  | { kind: "body-matches"; pattern: string }
  | { kind: "header-present"; name: string }
  | { kind: "header-missing"; name: string }
  | { kind: "last-failure" };

export interface ShrinkRequest {
  path: string;
  environment?: string;
  predicate: ShrinkPredicate;
  keep?: string[];
  max_probes?: number;
  max_seconds?: number;
  simplify_values?: boolean;
  confirmed_writes?: boolean;
}

export interface ShrinkStep {
  part: string;
  kind: string;
}

export interface ShrinkResult {
  request: RequestSpec;
  removed: ShrinkStep[];
  probes: number;
  not_reproduced?: boolean;
  flaky?: boolean;
  one_minimal?: boolean;
  stopped_by?: string;
}

// Consumer contracts (`rt_app::consumers`, strand F).
export interface ConsumerRef {
  name: string;
  source: string;
  ref?: string;
  path?: string;
  environment: string;
  vars?: [string, string][];
}

export interface ConsumerVerifyRequest {
  base_url: string;
  only?: string[];
  fetch?: boolean;
  keep_going?: boolean;
  trust_scripts?: boolean;
  allow_writes?: boolean;
  max_minutes?: number;
}

export interface ConsumerFailure {
  request_path: string;
  request: string;
  assertion: string;
  difference: string;
}

export interface ConsumerResult {
  name: string;
  commit?: string;
  requests: number;
  skipped?: number;
  failures: ConsumerFailure[];
  error?: string;
}

export interface ConsumerReport {
  consumers: ConsumerResult[];
}

// Sequence diagrams (`rt_app::sequence`, strand G).
export type ParticipantKind = "client" | "server" | "script" | "plugin" | "agent";

export interface SequenceParticipant {
  id: string;
  label: string;
  kind: ParticipantKind;
}

export interface SequenceMessage {
  kind: "request" | "response";
  from: string;
  to: string;
  label: string;
  request_path?: string;
}

export type SequenceNoteAnchor =
  | { kind: "over"; participant: string }
  | { kind: "between"; from: string; to: string };

export interface SequenceNote {
  after: number;
  anchor: SequenceNoteAnchor;
  text: string;
}

export interface SequenceGroup {
  kind: string;
  label: string;
  from: number;
  to: number;
}

export interface SequenceModel {
  participants: SequenceParticipant[];
  messages: SequenceMessage[];
  notes?: SequenceNote[];
  groups?: SequenceGroup[];
  omitted?: number;
}

export type SequenceSource =
  | { kind: "run"; run_id: number }
  | { kind: "session" }
  | { kind: "pages"; request_path: string }
  | { kind: "mcp"; since: number | null }
  | { kind: "history"; ids: number[] };

export interface SequenceOptions {
  include_query?: boolean;
  include_values?: boolean;
}

export interface SequenceText {
  mermaid: string;
  markdown: string;
}

// Passive API security checks (`rt_app::security`, strand R).
export type OwaspCategory =
  | "API1:2023"
  | "API2:2023"
  | "API3:2023"
  | "API4:2023"
  | "API5:2023"
  | "API6:2023"
  | "API7:2023"
  | "API8:2023"
  | "API9:2023"
  | "API10:2023";

export type SecuritySeverity = "info" | "low" | "medium" | "high" | "critical";

export interface SecurityRule {
  id: string;
  title: string;
  category: OwaspCategory;
  severity: SecuritySeverity;
  native_only?: boolean;
}

export interface SecurityFinding {
  rule: string;
  category: OwaspCategory;
  severity: SecuritySeverity;
  location: string;
  why: string;
  fix: string;
}

export interface SecurityTarget {
  path: string;
  response_id: number;
}

export interface SecuritySettings {
  enabled?: boolean;
  disabled?: string[];
}

export type AuditFormat = "text" | "markdown" | "html" | "json" | "sarif";

/** What the native transport learned about the TLS connection (`rt_core::transport::TlsInfo`). */
export interface TlsInfo {
  protocol?: string;
  cipher?: string;
  not_before?: number;
  not_after?: number;
  key_algorithm?: string;
  key_bits?: number;
  names?: string[];
}

// API notebooks (`rt_app::notebook`, strand W).
export type NotebookShow =
  | { kind: "response" }
  | { kind: "table"; path: string }
  | { kind: "chart"; path: string };

export type NotebookBlock =
  | { kind: "text"; markdown: string }
  | {
      kind: "request";
      id: string;
      ref?: string;
      embedded?: RequestSpec;
      set?: [string, string][];
      show?: NotebookShow[];
    };

export interface Notebook {
  title: string;
  blocks: NotebookBlock[];
}

export interface NotebookEntry {
  path: string;
  title: string;
  blocks: number;
}

export interface NotebookRun {
  path: string;
  block?: string;
  environment?: string;
  allow_writes?: boolean;
  /** Variables of earlier blocks the view holds (a masked value is not carried). */
  vars?: [string, string][];
}

export interface BlockView {
  show: NotebookShow;
  result: ViewResult;
}

export interface BlockResult {
  block: string;
  report?: SendReport;
  set?: [string, string][];
  skipped?: string;
  /** Tables and chart values of the block's `show:` lines (never saved). */
  views?: BlockView[];
  notes?: string[];
}

export interface NotebookOutcome {
  results: BlockResult[];
}

// The script debugger (`rt_script::debug`, strand Y).
export interface Breakpoint {
  line: number;
  condition?: string;
}

export interface DebugTarget {
  request_path: string;
  phase: string;
  folder?: string;
  environment?: string;
  /** A kept response, for `after-response` and `tests`. */
  response_id?: number;
  breakpoints: Breakpoint[];
  stop_on_entry?: boolean;
}

export type DebugCommand =
  | { kind: "continue" }
  | { kind: "step-over" }
  | { kind: "step-into" }
  | { kind: "step-out" }
  | { kind: "evaluate"; expression: string }
  | { kind: "set-breakpoints"; breakpoints: Breakpoint[] }
  | { kind: "abort" };

export interface DebugValue {
  name: string;
  kind: string;
  preview: string;
  children?: number;
}

export interface DebugFrame {
  function: string;
  line: number;
}

export interface DebugScope {
  name: string;
  values: DebugValue[];
}

export interface DebugPause {
  line: number;
  reason: string;
  frames: DebugFrame[];
  scopes: DebugScope[];
  evaluated?: DebugValue;
}

export type DebugStatus = "running" | "paused" | "finished" | "aborted";

export interface DebugState {
  session: number;
  status: DebugStatus;
  pause?: DebugPause;
  output?: string[];
  error?: string;
}

// ---- AI types moved to @tailor-kit/ai-ui (K3): same shapes, same names ----

import type { AiPolicy, AiRequest as KitAiRequest } from "@tailor-kit/ai-ui";

export type {
  AiCall,
  AiCapabilities,
  AiConfig,
  AiDelta,
  AiError,
  AiEvent,
  AiJsonMode,
  AiMessage,
  AiPreview,
  AiPreviewMode,
  AiProfile,
  AiProviderKind,
  AiReply,
  AiRole,
  AiRoles,
  AiSettings,
  AiStop,
  AiTool,
  AiToolCall,
  AiToolResult,
  AiUsage,
} from "@tailor-kit/ai-ui";

/** Wave 16: off if either is set (`AiPolicy` of the kit: here the active collection). */
export type AiCollectionPolicy = AiPolicy;

/** Wave 16: a call an extension asks for (`tk_ai::AiRequest<AiScope>`). */
export type AiRequest = KitAiRequest<AiScope>;
