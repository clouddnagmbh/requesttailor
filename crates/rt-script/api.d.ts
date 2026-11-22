// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The script API of RequestTailor (wave 7): what `before-request`, `after-response` and
// `tests` scripts see. One API on every surface (desktop, web, CLI, runner); the engine is
// QuickJS in a sandbox (crates/rt-script). `bru.*`, `req`, `res` (Bruno) and `pm.*`
// (Postman) are shims on top of this API; their coverage is in SHIM-COVERAGE.md.
//
// Rules of the sandbox:
// - No file system, no network except `rt.sendRequest`, no timers except `rt.sleep`, no
//   `require` of modules except the built-in ones listed below, no `eval` of host code.
// - Time limit (default 5 s per phase and request, `rt.sleep` included) and memory limit
//   (default 64 MiB); hitting one stops the script with an error, the app stays usable.
// - Scripts see secret values (a pre-request script may sign with a key). What leaves a
//   script (console output, test names and messages, variables it sets) is masked in the
//   report like every other secret.
// - Scripts run in order: collection, folders (outermost first), request. All scripts of
//   one phase share one global scope; each phase starts fresh.
// - Scripts are synchronous code; `await` works at the top level (for `sendRequest` and
//   `sleep`), the phase ends when the last promise settles.
//
// Changing this file is an interface change (working rule 1).

declare global {
  const rt: Rt;

  /** Defines a test (phase `tests`; also allowed in `after-response`). A thrown error or a
   *  failed `expect` fails it; an async function is awaited. */
  function test(name: string, fn: () => void | Promise<void>): void;

  /** Chai-style assertions (the subset listed in `Expect`). */
  function expect(value: unknown, message?: string): Expect;

  /** Output goes into the report (`SendReport.logs`), secrets masked. */
  const console: {
    log(...args: unknown[]): void;
    info(...args: unknown[]): void;
    warn(...args: unknown[]): void;
    error(...args: unknown[]): void;
    debug(...args: unknown[]): void;
  };

  /** Built-in modules for `require` (Postman and Bruno scripts use them):
   *  `crypto-js`, `uuid`, `lodash` (subset), `moment` (subset), `atob`/`btoa` as globals,
   *  `chai` (as `expect`). Strand B decides by corpus frequency; see SHIM-COVERAGE.md. */
  function require(name: string): unknown;
}

export interface Rt {
  /** The request. Writable in `before-request` (changes go on the wire after variables are
   *  resolved, so `{{templates}}` set here are resolved too); read-only later (what was sent,
   *  resolved, secrets as values). */
  readonly request: RtRequest;
  /** The response; `undefined` in `before-request`. */
  readonly response: RtResponse | undefined;
  readonly vars: RtVars;
  readonly info: RtInfo;
  /** Sends a request through the same transport (no scripts, plugins or inherited auth run
   *  for it). Counts against the time limit. */
  sendRequest(request: RtSendRequest): Promise<RtResponse>;
  /** Waits `ms` milliseconds (counts against the time limit). */
  sleep(ms: number): Promise<void>;
  /** Controls the collection runner; outside a run `skip()` still skips this send. */
  readonly runner: RtRunner;
}

export interface RtRequest {
  method: string;
  url: string;
  /** Enabled headers in order; names keep their case. */
  readonly headers: RtHeaders;
  /** Text body (JSON, text, XML); `undefined` for none or a body scripts cannot edit (form,
   *  multipart, file). Setting it on a request without a body makes it raw (JSON if it
   *  parses). */
  body: string | undefined;
  /** `JSON.parse(body)`; throws if the body is not JSON. */
  json(): unknown;
}

export interface RtHeaders {
  /** Case-insensitive; the first match. */
  get(name: string): string | undefined;
  has(name: string): boolean;
  /** Replaces all headers of that name (case-insensitive) with one. */
  set(name: string, value: string): void;
  /** Adds another header with that name. */
  add(name: string, value: string): void;
  delete(name: string): void;
  /** All headers as `[name, value]` pairs, in order. */
  entries(): [string, string][];
  /** Last value per lower-cased name. */
  toObject(): Record<string, string>;
}

export interface RtResponse {
  readonly status: number;
  readonly statusText: string;
  readonly headers: Pick<RtHeaders, "get" | "has" | "entries" | "toObject">;
  /** The body as text (UTF-8, lossy). */
  text(): string;
  /** `JSON.parse(text())`, cached; throws if the body is not JSON. */
  json(): unknown;
  /** Milliseconds from sending until the last byte. */
  readonly time: number;
  /** Bytes received. */
  readonly size: number;
  /** URL after redirects. */
  readonly url: string;
}

export type RtVarScope = "runtime" | "environment" | "collection" | "global";

export interface RtVars {
  /** The value as the request would resolve it (all layers, highest precedence first);
   *  `undefined` if unset. */
  get(name: string): string | undefined;
  has(name: string): boolean;
  /** Sets a variable. Values are strings: other values are stored as JSON
   *  (`JSON.stringify`), objects included. Default scope `runtime` (this send, the session in
   *  the app, the run in the runner). `environment`, `collection`, `global` change that layer
   *  in memory for the session or run; files are never written. */
  set(name: string, value: unknown, scope?: RtVarScope): void;
  unset(name: string, scope?: RtVarScope): void;
  /** Replaces every `{{name}}` in `template` with the current values. */
  replace(template: string): string;
  /** Every variable in scope, highest precedence wins. */
  toObject(): Record<string, string>;
}

export interface RtInfo {
  readonly requestName: string;
  /** Path relative to the collection root, `/`-separated. */
  readonly requestPath: string;
  readonly collectionName: string;
  /** The selected environment, `undefined` for none. */
  readonly environment: string | undefined;
  readonly phase: "before-request" | "after-response" | "tests";
  /** 0-based iteration of the runner, 0 outside a run. */
  readonly iteration: number;
  /** Iterations of the run, 1 outside a run. */
  readonly iterationCount: number;
  /** The data row of this iteration (CSV columns as strings, JSON values as they are);
   *  `{}` outside a data-driven run. */
  readonly iterationData: Record<string, unknown>;
}

export interface RtSendRequest {
  method?: string;
  url: string;
  headers?: Record<string, string> | [string, string][];
  /** A string is sent as is; anything else as JSON with `Content-Type: application/json`. */
  body?: unknown;
}

export interface RtRunner {
  /** `before-request`: do not send this request (counted as skipped). */
  skip(): void;
  /** Continue the run with the request named `name` (name or path); `null` stops the run. */
  setNext(name: string | null): void;
}

/** The chai subset (`expect(x).to.equal(y)`, …). Chains are words without effect: `to`, `be`,
 *  `been`, `is`, `that`, `which`, `and`, `has`, `have`, `with`, `at`, `of`, `same`, `but`,
 *  `does`, `still`. `not` and `deep` change the assertion that follows. */
export interface Expect {
  readonly to: Expect;
  readonly be: Expect;
  readonly been: Expect;
  readonly is: Expect;
  readonly that: Expect;
  readonly which: Expect;
  readonly and: Expect;
  readonly has: Expect;
  readonly have: Expect;
  readonly with: Expect;
  readonly at: Expect;
  readonly of: Expect;
  readonly not: Expect;
  readonly deep: Expect;

  readonly ok: void;
  readonly true: void;
  readonly false: void;
  readonly null: void;
  readonly undefined: void;
  readonly exist: void;
  readonly empty: void;

  equal(value: unknown, message?: string): Expect;
  equals(value: unknown, message?: string): Expect;
  eq(value: unknown, message?: string): Expect;
  eql(value: unknown, message?: string): Expect;
  above(n: number, message?: string): Expect;
  gt(n: number, message?: string): Expect;
  least(n: number, message?: string): Expect;
  gte(n: number, message?: string): Expect;
  below(n: number, message?: string): Expect;
  lt(n: number, message?: string): Expect;
  most(n: number, message?: string): Expect;
  lte(n: number, message?: string): Expect;
  within(low: number, high: number, message?: string): Expect;
  a(type: string, message?: string): Expect;
  an(type: string, message?: string): Expect;
  include(value: unknown, message?: string): Expect;
  includes(value: unknown, message?: string): Expect;
  contain(value: unknown, message?: string): Expect;
  contains(value: unknown, message?: string): Expect;
  oneOf(list: unknown[], message?: string): Expect;
  property(name: string, value?: unknown, message?: string): Expect;
  lengthOf(n: number, message?: string): Expect;
  length(n: number, message?: string): Expect;
  match(re: RegExp, message?: string): Expect;
  string(s: string, message?: string): Expect;
  keys(...names: string[]): Expect;
  members(list: unknown[], message?: string): Expect;
  instanceOf(ctor: unknown, message?: string): Expect;
  throw(error?: unknown, message?: string): Expect;
  satisfy(fn: (value: unknown) => boolean, message?: string): Expect;
  /** Postman/Bruno: `expect(res).to.have.status(200)`. */
  status(code: number): Expect;
  header(name: string, value?: string): Expect;
  jsonBody(path?: string, value?: unknown): Expect;
}
