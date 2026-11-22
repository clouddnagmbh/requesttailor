// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A stand-in for the script engine (wave 7, strand A) under Node, so the shims can be tested
// before the QuickJS sandbox exists: `rt` as in crates/rt-script/api.d.ts, `test`, `console`,
// `require` of crates/rt-script/js/modules, the shims of crates/rt-script/js/shims loaded in
// file-name order, and a runner for the case files of crates/rt-script/tests/cases (format:
// see the strand A/B case-file description; mirrors rt_core::script::{ScriptCall,
// ScriptOutcome}).
//
// It runs in a fresh `node:vm` context per call: only ECMAScript built-ins, no Node APIs.
// Time is virtual: `rt.sleep` and `setTimeout` advance a clock instead of waiting.

import { readdirSync, readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import vm from "node:vm";

const JS = join(dirname(fileURLToPath(import.meta.url)), "..");
export const SHIMS_DIR = join(JS, "shims");
export const MODULES_DIR = join(JS, "modules");
export const CASES_DIR = join(JS, "../tests/cases");

const RANK = { global: 0, collection: 1, environment: 2, folder: 3, request: 4, runtime: 5 };
const SCOPES = ["runtime", "environment", "collection", "global"];

const REASONS = {
  100: "Continue", 101: "Switching Protocols", 200: "OK", 201: "Created", 202: "Accepted",
  203: "Non-Authoritative Information", 204: "No Content", 206: "Partial Content",
  301: "Moved Permanently", 302: "Found", 303: "See Other", 304: "Not Modified",
  307: "Temporary Redirect", 308: "Permanent Redirect", 400: "Bad Request", 401: "Unauthorized",
  403: "Forbidden", 404: "Not Found", 405: "Method Not Allowed", 406: "Not Acceptable",
  409: "Conflict", 410: "Gone", 415: "Unsupported Media Type", 422: "Unprocessable Entity",
  429: "Too Many Requests", 500: "Internal Server Error", 501: "Not Implemented",
  502: "Bad Gateway", 503: "Service Unavailable", 504: "Gateway Timeout",
};

/** Names of the shim files in load order. */
export function shimFiles() {
  return readdirSync(SHIMS_DIR).filter((f) => f.endsWith(".js")).sort();
}

class ScriptError extends Error {}

/** Values handed to scripts are made in the script's realm (so `instanceof Object` and
 *  friends behave as in the engine). */
let realm = JSON;
const own = (v) => (v === undefined ? undefined : realm.parse(JSON.stringify(v)));

function headersApi(list, writable, guard) {
  const lower = (s) => String(s).toLowerCase();
  const api = {
    get(name) {
      const h = list.find(([n]) => lower(n) === lower(name));
      return h ? h[1] : undefined;
    },
    has(name) {
      return list.some(([n]) => lower(n) === lower(name));
    },
    entries() {
      return own(list);
    },
    toObject() {
      const o = {};
      for (const [n, v] of list) o[lower(n)] = v;
      return own(o);
    },
  };
  if (writable) {
    Object.assign(api, {
      set(name, value) {
        guard();
        const i = list.findIndex(([n]) => lower(n) === lower(name));
        const rest = list.filter(([n], k) => k === i || lower(n) !== lower(name));
        list.length = 0;
        list.push(...rest);
        if (i < 0) list.push([String(name), String(value)]);
        else list[list.findIndex(([n]) => lower(n) === lower(name))] = [String(name), String(value)];
      },
      add(name, value) {
        guard();
        list.push([String(name), String(value)]);
      },
      delete(name) {
        guard();
        const rest = list.filter(([n]) => lower(n) !== lower(name));
        list.length = 0;
        list.push(...rest);
      },
    });
  }
  return api;
}

function decodeBody(r) {
  if (r.body_base64 !== undefined) return Buffer.from(r.body_base64, "base64");
  return Buffer.from(r.body ?? "", "utf8");
}

function responseApi(r) {
  const bytes = decodeBody(r);
  const text = bytes.toString("utf8");
  let cached;
  return Object.freeze({
    status: r.status,
    statusText: REASONS[r.status] ?? "",
    headers: headersApi((r.headers ?? []).map(([n, v]) => [n, v]), false),
    text: () => text,
    json: () => (cached ??= { v: realm.parse(text) }).v,
    time: r.time_ms ?? 0,
    size: bytes.length,
    url: r.url ?? "",
  });
}

function format(v) {
  if (typeof v === "string") return v;
  if (v instanceof Error) return String(v);
  try {
    const s = JSON.stringify(v);
    return s === undefined ? String(v) : s;
  } catch {
    return String(v);
  }
}

/** Runs one phase: `call` has the case-file shape (`phase`, `scripts`, `request`, …).
 *  Resolves to `{ request, var_changes, tests, logs, flow, error, sent }` (`sent`: what
 *  `rt.sendRequest` sent, for tests). */
export async function runPhase(call) {
  const phase = call.phase;
  const before = phase === "before-request";
  const limit = call.limits?.time_ms ?? 5000;
  const out = { request: null, var_changes: [], tests: [], logs: [], flow: { kind: "continue" }, error: null, sent: [] };
  let clock = 0;
  const pending = new Set();
  const track = (p) => {
    pending.add(p);
    p.then(
      () => pending.delete(p),
      () => pending.delete(p),
    );
    return p;
  };
  const elapse = (ms) => {
    clock += Math.max(0, Number(ms) || 0);
    if (clock > limit) throw new ScriptError(`script time limit of ${limit} ms exceeded`);
  };
  const tick = () => new Promise((r) => setImmediate(r));
  const ctx = vm.createContext({});
  realm = vm.runInContext("JSON", ctx);

  // Request
  const req0 = call.request;
  const req = { method: req0.method, url: req0.url, headers: (req0.headers ?? []).map(([n, v]) => [n, v]), body: req0.body ?? undefined };
  const guard = () => {
    if (!before) throw new TypeError("rt.request is read-only after the request was sent");
  };
  const request = {
    get method() {
      return req.method;
    },
    set method(v) {
      guard();
      req.method = String(v);
    },
    get url() {
      return req.url;
    },
    set url(v) {
      guard();
      req.url = String(v);
    },
    headers: headersApi(req.headers, true, guard),
    get body() {
      return req.body;
    },
    set body(v) {
      guard();
      req.body = v === undefined ? undefined : String(v);
    },
    json() {
      return realm.parse(req.body);
    },
  };

  // Variables
  const vars = (call.variables ?? []).map((v) => ({ name: v.name, value: v.value, rank: RANK[v.source.layer], secret: !!v.secret }));
  const lookup = (name) => {
    let best;
    for (const v of vars) if (v.name === name && (!best || v.rank >= best.rank)) best = v;
    return best;
  };
  const scopeOf = (scope) => {
    const s = scope ?? "runtime";
    if (!SCOPES.includes(s)) throw new TypeError(`unknown variable scope '${s}'`);
    return s;
  };
  const rtVars = {
    get: (name) => lookup(String(name))?.value,
    has: (name) => lookup(String(name)) !== undefined,
    set(name, value, scope) {
      const s = scopeOf(scope);
      const text = typeof value === "string" ? value : JSON.stringify(value) ?? String(value);
      const existing = vars.find((v) => v.name === name && v.rank === RANK[s]);
      if (existing) existing.value = text;
      else vars.push({ name: String(name), value: text, rank: RANK[s], secret: false });
      out.var_changes.push({ name: String(name), value: text, scope: s });
    },
    unset(name, scope) {
      const s = scopeOf(scope);
      for (let i = vars.length - 1; i >= 0; i--) if (vars[i].name === name && vars[i].rank === RANK[s]) vars.splice(i, 1);
      out.var_changes.push({ name: String(name), value: null, scope: s });
    },
    replace: (template) => String(template).replace(/\{\{([^{}]+)\}\}/g, (m, n) => lookup(n.trim())?.value ?? m),
    toObject() {
      const o = {};
      for (const v of [...vars].sort((a, b) => a.rank - b.rank)) o[v.name] = v.value;
      return own(o);
    },
  };

  const it = call.info?.iteration;
  const info = Object.freeze({
    requestName: call.info?.request_name ?? "",
    requestPath: call.info?.request_path ?? "",
    collectionName: call.info?.collection_name ?? "",
    environment: call.info?.environment ?? undefined,
    phase,
    iteration: it?.index ?? 0,
    iterationCount: it?.count ?? 1,
    iterationData: Object.freeze(own(Object.fromEntries(it?.data ?? []))),
  });

  const response = call.response ? responseApi(call.response) : undefined;
  const sendRequest = (r) =>
    track(
      (async () => {
        await tick();
        if (r == null || typeof r.url !== "string") throw new TypeError("rt.sendRequest needs { url }");
        let headers = r.headers ?? [];
        if (!Array.isArray(headers)) headers = Object.entries(headers);
        headers = headers.map(([n, v]) => [String(n), String(v)]);
        let body = r.body;
        if (body !== undefined && body !== null && typeof body !== "string") {
          body = JSON.stringify(body);
          if (!headers.some(([n]) => n.toLowerCase() === "content-type")) headers.push(["Content-Type", "application/json"]);
        }
        const sent = { method: (r.method ?? "GET").toUpperCase(), url: r.url, headers, body: body ?? null };
        out.sent.push(sent);
        const canned = (call.send ?? []).find((s) => s.url === r.url);
        if (!canned) throw new Error(`no canned response for ${r.url}`);
        if (canned.error) throw new Error(canned.error);
        elapse(canned.response.time_ms ?? 0);
        return responseApi(canned.response);
      })(),
    );
  // Sleeps wake in virtual-time order (see `drain`).
  const timers = [];
  let seq = 0;
  const sleep = (ms) => track(new Promise((resolve) => timers.push({ due: clock + Math.max(0, Number(ms) || 0), seq: seq++, resolve })));
  /** Runs until nothing is pending: microtasks first, then the earliest sleep. */
  const drain = async () => {
    for (let guard = 0; guard < 100000; guard++) {
      for (let i = 0; i < 5; i++) await tick();
      if (timers.length) {
        timers.sort((a, b) => a.due - b.due || a.seq - b.seq);
        const t = timers.shift();
        elapse(t.due - clock);
        t.resolve();
        continue;
      }
      if (!pending.size) return;
      await Promise.race([...pending].map((p) => p.catch(() => {})));
    }
  };
  const rt = Object.freeze({
    request: Object.freeze(request),
    response,
    vars: Object.freeze(rtVars),
    info,
    sendRequest,
    sleep,
    runner: Object.freeze({
      skip() {
        if (before) out.flow = { kind: "skip" };
      },
      setNext(name) {
        out.flow = { kind: "next", request: name === null || name === undefined ? null : String(name) };
      },
    }),
  });

  // Context
  const define = (name, value) => Object.defineProperty(ctx, name, { value, writable: true, configurable: true, enumerable: false });
  let origin = "";
  define("rt", rt);
  define("console", Object.freeze(Object.fromEntries(["log", "info", "warn", "error", "debug"].map((level) => [level, (...args) => void out.logs.push({ level, text: args.map(format).join(" "), origin })]))));
  define("test", (name, fn) => {
    const entry = { name: String(name), passed: true, origin };
    out.tests.push(entry);
    const fail = (e) => {
      entry.passed = false;
      entry.error = e && e.message !== undefined ? String(e.message) : String(e);
    };
    try {
      const r = fn();
      if (r && typeof r.then === "function") track(Promise.resolve(r).then(() => {}, fail));
    } catch (e) {
      fail(e);
    }
  });
  const modules = new Map();
  const requireFn = (name) => {
    name = String(name);
    if (modules.has(name)) return modules.get(name).exports;
    let code;
    if (/^[\w.-]+$/.test(name)) {
      try {
        code = readFileSync(join(MODULES_DIR, `${name}.js`), "utf8");
      } catch {
        code = undefined;
      }
    }
    if (code === undefined) throw new Error(`Cannot find module '${name}'`);
    const module = { exports: {} };
    modules.set(name, module);
    vm.runInContext(`(function (module, exports, require) {${code}\n})`, ctx, { filename: `modules/${name}.js` })(module, module.exports, requireFn);
    return module.exports;
  };
  define("require", requireFn);

  // Shims
  for (const f of shimFiles()) vm.runInContext(readFileSync(join(SHIMS_DIR, f), "utf8"), ctx, { filename: `shims/${f}` });

  // Scripts
  const rejections = [];
  const onRejection = (reason) => rejections.push(reason);
  process.on("unhandledRejection", onRejection);
  try {
    for (const s of call.scripts) {
      origin = s.origin ?? "request";
      const code = /\bawait\b/.test(s.code) ? `(async () => {\n${s.code}\n})()` : s.code;
      const r = vm.runInContext(code, ctx, { filename: origin });
      let failed;
      if (r && typeof r.then === "function" && /\bawait\b/.test(s.code)) track(Promise.resolve(r).catch((e) => (failed = { e })));
      // Let the script's promises settle before the next one (phase ends at the last).
      await drain();
      if (failed) throw failed.e;
      if (rejections.length) throw rejections[0];
    }
  } catch (e) {
    out.error = e && e.message !== undefined ? `${e.name ?? "Error"}: ${e.message}` : String(e);
  } finally {
    process.off("unhandledRejection", onRejection);
  }

  const changed = req.method !== req0.method || req.url !== req0.url || (req.body ?? null) !== (req0.body ?? null) || JSON.stringify(req.headers) !== JSON.stringify(req0.headers ?? []);
  if (before && changed) out.request = { method: req.method, url: req.url, headers: req.headers, body: req.body ?? null };
  return out;
}

/** Differences between `outcome` and `expected` (the case's `expect`), as text lines. */
export function compareOutcome(outcome, expected = {}) {
  const diffs = [];
  const eq = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  if ("request" in expected && !eq(outcome.request, expected.request))
    diffs.push(`request: got ${JSON.stringify(outcome.request)}, want ${JSON.stringify(expected.request)}`);
  if ("var_changes" in expected && !eq(outcome.var_changes, expected.var_changes))
    diffs.push(`var_changes: got ${JSON.stringify(outcome.var_changes)}, want ${JSON.stringify(expected.var_changes)}`);
  if ("tests" in expected) {
    const got = outcome.tests;
    const want = expected.tests;
    const ok =
      got.length === want.length &&
      want.every((w, i) => got[i].name === w.name && got[i].passed === w.passed && (w.error === undefined || (got[i].error ?? "").includes(w.error)));
    if (!ok) diffs.push(`tests: got ${JSON.stringify(got.map(({ name, passed, error }) => ({ name, passed, error })))}, want ${JSON.stringify(want)}`);
  }
  if ("logs" in expected) {
    const got = outcome.logs.map(({ level, text }) => ({ level, text }));
    if (!eq(got, expected.logs)) diffs.push(`logs: got ${JSON.stringify(got)}, want ${JSON.stringify(expected.logs)}`);
  }
  if ("flow" in expected && !eq(outcome.flow, expected.flow)) diffs.push(`flow: got ${JSON.stringify(outcome.flow)}, want ${JSON.stringify(expected.flow)}`);
  if ("error" in expected) {
    if (expected.error === null) {
      if (outcome.error !== null) diffs.push(`error: got ${outcome.error}, want none`);
    } else if (!(outcome.error ?? "").includes(expected.error)) diffs.push(`error: got ${outcome.error}, want …${expected.error}…`);
  }
  return diffs;
}

/** Every case file below `dir` (recursively): `[{ file, case }]`. */
export function caseFiles(dir) {
  const out = [];
  const walk = (d) => {
    for (const e of readdirSync(d, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name.endsWith(".json")) out.push({ file: p, case: JSON.parse(readFileSync(p, "utf8")) });
    }
  };
  walk(dir);
  return out;
}

/** A case with defaults for quick unit tests: `phase` "tests", a GET request, a 200 JSON
 *  response unless `response: null`. */
export function quick(code, extra = {}) {
  const phase = extra.phase ?? "tests";
  return {
    phase,
    scripts: [{ origin: "request", code }],
    request: { method: "GET", url: "http://h/x", headers: [], body: null },
    response: phase === "before-request" ? undefined : { status: 200, headers: [["Content-Type", "application/json"]], body: '{"id":7}', time_ms: 12, url: "http://h/x" },
    ...extra,
  };
}
