// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The plugin API, version 1, as TypeScript types: a mirror of `crates/rt-plugin/src/api.rs`
// (the JSON, exactly as serde writes it). `crates/rt-plugin/API.md` describes the rules.
//
// Fields the Rust side fills in with a default when they are missing are optional here, so
// a plugin may leave them out of its replies. `contract.generated.ts` holds real JSON from
// the Rust types, annotated with these types: `npm run check -w sdk/typescript` fails when
// they drift apart. Regenerate it with
// `RT_UPDATE_CONTRACT=1 cargo test -p rt-plugin-sdk --test ts_contract`.
//
// The request model (`RequestSpec`) and the loss report are the app's types; they are
// mirrored once, in the UI, and checked by the app's own contract test.

import type { LossReport, NoteLevel, RequestSpec } from "../../../apps/ui/src/commands/types.ts";

export type {
  Auth,
  Body,
  ImportFormat,
  KeyValue,
  Loss,
  LossEffect,
  LossKind,
  LossReport,
  NoteLevel,
  Param,
  RequestSpec,
  Settings,
  Variable,
} from "../../../apps/ui/src/commands/types.ts";

/** The API version this SDK is written for (`Manifest.api`). */
export const API_VERSION = 1;

/** The WebAssembly custom section that holds the manifest as UTF-8 JSON. */
export const MANIFEST_SECTION = "rt-plugin";

/** A pair list, e.g. headers: `[name, value][]`. */
export type Pairs = [string, string][];

// ---- manifest ----

export interface Manifest {
  /** `[a-z0-9][a-z0-9-]*`, at most 64 characters. */
  id: string;
  name: string;
  /** Semantic version of the plugin. */
  version: string;
  /** Plugin API version the plugin was built for. */
  api: number;
  description?: string;
  /** SPDX expression. */
  license?: string | null;
  homepage?: string | null;
  permissions?: Permissions;
  contributes?: Contributions;
}

export interface Permissions {
  /** Exact hosts, `*.example.com`, `{request}` or `*`. Empty: no network. */
  network?: string[];
  storage?: boolean;
  variables?: boolean;
}

export interface Contributions {
  prepare?: boolean;
  response?: boolean;
  tabs?: TabContribution[];
  importers?: ImporterContribution[];
  /** First-start choice (bundled plugins only). Wave 6. */
  welcome?: WelcomeChoice | null;
  /** Rules for following pages of responses, as data. Wave 18. */
  paging?: PagingContribution[];
  /** Rules that say how writing requests are put back, as data. Wave 19. */
  compensations?: CompensationContribution[];
}

/** Wave 19: how writing requests to matching hosts are undone; the app executes, the plugin
 *  supplies data only. */
export interface CompensationContribution {
  id: string;
  match: CompensationMatch;
  rule: CompensationRule;
}

/** Empty lists mean "any". */
export interface CompensationMatch {
  hosts?: string[];
  content_types?: string[];
  /** A glob over the request URL's path and query. */
  url_pattern?: string | null;
  /** Upper-case methods; empty: all writing methods. */
  methods?: string[];
}

export interface CompensationRule {
  /** Where a created resource is named, tried in order: `header:<Name>` or a JSONPath. */
  id_from?: string[];
  /** URL of one resource: `{collection}` and `{id}`. */
  resource_url?: string | null;
  /** Where the version for `If-Match` is, tried in order. */
  etag_from?: string[];
  /** A token to fetch before a writing call. */
  token?: { fetch_header: string; fetch_value: string; read_header: string; send_header: string } | null;
  /** Entries this rule cannot undo, with a reason. */
  not_reversible?: { url_pattern: string; reason: string }[];
}

/** Wave 18: a paging rule a plugin ships; a rule of the collection or request wins. */
export interface PagingContribution {
  id: string;
  match: PagingMatch;
  rule: PagingSpec;
}

/** Empty lists mean "any". */
export interface PagingMatch {
  hosts?: string[];
  content_types?: string[];
  /** A glob over the request URL's path and query. */
  url_pattern?: string | null;
}

export interface PagingSpec {
  rule: PagingRule;
  /** JSONPath of the array whose items of all pages are appended to the first page's. */
  merge?: string | null;
}

export type PagingRule =
  | { kind: "link-header" }
  | { kind: "json-url"; next: string }
  | { kind: "cursor"; next: string; param: { place: "query" | "body" | "header"; name: string } };

export interface WelcomeChoice {
  label: string;
  description?: string;
}

/** `collection`: a panel next to the request tree; `target.request_path` is "". Wave 6. */
export type TabPlace = "request" | "response" | "collection";

export interface TabContribution {
  id: string;
  title: string;
  place: TabPlace;
  /** Lowercase prefixes of the response `Content-Type`. Empty: always. */
  content_types?: string[];
}

export interface ImporterContribution {
  id: string;
  label: string;
  /** Without the dot, lowercase. */
  extensions?: string[];
}

// ---- data handed to plugins (secret values are `***`) ----

export interface PluginRequest {
  method: string;
  url: string;
  headers: Pairs;
  body?: PluginBody | null;
}

export interface PluginResponse {
  status: number;
  headers: Pairs;
  /** Final URL after redirects. */
  url: string;
  body: PluginBody;
  /** The body was cut at the host's limit. */
  truncated: boolean;
}

/** UTF-8 text as `text`, anything else as standard base64 in `base64`. */
export interface PluginBody {
  content_type?: string | null;
  text?: string | null;
  base64?: string | null;
}

export interface PluginContext {
  /** Path of the request file, relative to the collection root. */
  request_path: string;
  environment?: string | null;
  /** Non-secret variables; empty without the `variables` permission. */
  variables?: Pairs;
}

export interface TabTarget {
  request_path: string;
  response_id?: number | null;
}

export interface PluginFile {
  name: string;
  /** Standard base64. */
  base64: string;
}

// ---- host → plugin: the argument of `rt_call` ----

export type Call =
  | { hook: "prepare"; request: PluginRequest; context: PluginContext }
  | { hook: "response"; request: PluginRequest; response: PluginResponse; context: PluginContext }
  | {
      hook: "render-tab";
      tab: string;
      target: TabTarget;
      spec: RequestSpec;
      response?: PluginResponse | null;
      state?: unknown;
      /** Wave 6; older hosts send none. */
      context?: PluginContext | null;
    }
  | {
      hook: "action";
      tab: string;
      action: string;
      target: TabTarget;
      spec: RequestSpec;
      response?: PluginResponse | null;
      state?: unknown;
      /** Form values by field name (forms only). */
      input?: Pairs;
      /** Wave 6; older hosts send none. */
      context?: PluginContext | null;
    }
  | { hook: "detect"; importer: string; files: PluginFile[] }
  | { hook: "import"; importer: string; files: PluginFile[] };

export type Hook = Call["hook"];

/** What `rt_call` (and `rt.host`) returns: `ok` or `error`. */
export type Reply<T> = { ok: T; error?: null } | { ok?: null; error: string };

export interface PrepareReply {
  patch?: RequestPatch;
  notes?: PluginNote[];
}

/** Applied in this order: `url`, `remove_headers`, `set_headers`, `body`. No `***` values. */
export interface RequestPatch {
  url?: string | null;
  remove_headers?: string[];
  set_headers?: Pairs;
  body?: PluginBody | null;
}

export interface NotesReply {
  notes?: PluginNote[];
  /** Parts of the server's time, shown under the timing bar, never part of its sum. Wave 18. */
  segments?: TimingSegment[];
}

export interface TimingSegment {
  id: string;
  label: string;
  ms: number;
  /** `"server"`: inside the wait for the server; since wave 19 also the `id` of another segment. */
  parent?: string | null;
  /** Wave 19: milliseconds after the server began handling the request. */
  offset_ms?: number | null;
}

export interface PluginNote {
  level: NoteLevel;
  text: string;
}

export interface ViewReply {
  view: View;
  /** Opaque to the host; passed back with the next call for the tab. */
  state?: unknown;
  /** Replaces the request in the editor (request tabs only). */
  update_request?: RequestSpec | null;
  /** New requests for the collection. Wave 6. */
  create_requests?: NewRequest[];
  /** New environments, non-secret values only. Wave 6. */
  add_environments?: PluginEnvironment[];
}

export interface NewRequest {
  /** Relative to the collection root, "" for the root. */
  folder?: string;
  name: string;
  spec: RequestSpec;
}

export interface PluginEnvironment {
  name: string;
  variables?: PluginVariable[];
}

export interface PluginVariable {
  name: string;
  value?: string;
  /** A secret without a value; the user sets it in Environments. */
  secret?: boolean;
}

export interface DetectReply {
  matches: boolean;
}

export interface ImportReply {
  collection_name: string;
  /** OpenCollection files: `[path, text][]`. */
  files: Pairs;
  report: LossReport;
}

// ---- views ----

export interface View {
  blocks: Block[];
}

export type Block =
  | { type: "heading"; text: string }
  | { type: "text"; text: string }
  | { type: "notice"; level: NoteLevel; text: string }
  | { type: "key-value"; rows: Pairs }
  | { type: "table"; columns: string[]; rows: string[][] }
  | { type: "code"; language?: string | null; text: string }
  | { type: "tree"; nodes: TreeItem[] }
  | { type: "form"; id: string; fields: FormField[]; submit: string }
  | { type: "actions"; actions: ActionButton[] };

export interface TreeItem {
  label: string;
  detail?: string | null;
  children?: TreeItem[];
  /** Clicking the item runs this action. */
  action?: string | null;
}

export type FieldKind = "text" | "select" | "checkbox";

export interface FormField {
  name: string;
  label: string;
  kind: FieldKind;
  value?: string;
  /** For `select`. */
  options?: string[];
}

export interface ActionButton {
  /** Passed as `action` in the `action` call. */
  id: string;
  label: string;
}

// ---- plugin → host: the argument of the `rt.host` import ----

export type HostCall =
  | { op: "log"; level: NoteLevel; message: string }
  | { op: "http"; request: PluginRequest; credentials?: boolean; session?: boolean }
  | { op: "storage-get"; key: string }
  | { op: "storage-set"; key: string; value: unknown }
  | { op: "storage-delete"; key: string }
  | { op: "now" };
