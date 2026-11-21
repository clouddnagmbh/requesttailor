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
  | { type: "unsupported"; kind: string };

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
  | { kind: "transport"; message: TransportError };

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
  text: string;
}

export type AppError =
  | { kind: "format"; message: string }
  | { kind: "request-not-found"; message: string }
  | { kind: "folder-not-found"; message: string }
  | { kind: "response-not-found"; message: number }
  | { kind: "environment-not-found"; message: string }
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
