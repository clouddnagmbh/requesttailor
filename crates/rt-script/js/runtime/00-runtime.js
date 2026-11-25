// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The runtime glue of the script engine (strand A): builds `rt`, `test`, `console` and
// `require` for one phase, runs the macrotask queue (`rt.sleep`, `rt.sendRequest`) and
// collects the outcome. Runs once when the engine starts (engine/src/engine.c, `rt_init`);
// the host snapshots the engine after that, so every phase starts from this state.
//
// Contract with the engine (C): `globalThis.__rt_internal` is this file's internal object
// (hidden from scripts after init) with `addModule(name, factory)`, `begin(input)`,
// `enter(origin)`, `scriptPromise(p)`, `tick() -> 0 | 1 | 2`, and
// `finish(error, stopped, rejections) -> JSON`. `__rt_host(json) -> json` is the suspending
// host call (`{op: "send", request}` / `{op: "sleep", ms}`).
//
// Script API: crates/rt-script/api.d.ts. Shims (js/shims, strand B) run after `begin`, as
// classic scripts in file-name order; `require(name)` loads js/modules/<name>.js.

(function () {
  "use strict";

  const host = globalThis.__rt_host;
  const realRandom = Math.random;
  const json = JSON;
  const now = () => performance.now();

  /** Most log entries and log text kept per phase. */
  const MAX_LOGS = 1000;
  const MAX_LOG_TEXT = 1 << 20;

  const STATUS_TEXT = {
    100: "Continue", 101: "Switching Protocols", 102: "Processing", 103: "Early Hints",
    200: "OK", 201: "Created", 202: "Accepted", 203: "Non-Authoritative Information",
    204: "No Content", 205: "Reset Content", 206: "Partial Content", 207: "Multi-Status",
    208: "Already Reported", 226: "IM Used",
    300: "Multiple Choices", 301: "Moved Permanently", 302: "Found", 303: "See Other",
    304: "Not Modified", 305: "Use Proxy", 307: "Temporary Redirect", 308: "Permanent Redirect",
    400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden",
    404: "Not Found", 405: "Method Not Allowed", 406: "Not Acceptable",
    407: "Proxy Authentication Required", 408: "Request Timeout", 409: "Conflict", 410: "Gone",
    411: "Length Required", 412: "Precondition Failed", 413: "Content Too Large",
    414: "URI Too Long", 415: "Unsupported Media Type", 416: "Range Not Satisfiable",
    417: "Expectation Failed", 418: "I'm a Teapot", 421: "Misdirected Request",
    422: "Unprocessable Content", 423: "Locked", 424: "Failed Dependency", 425: "Too Early",
    426: "Upgrade Required", 428: "Precondition Required", 429: "Too Many Requests",
    431: "Request Header Fields Too Large", 451: "Unavailable For Legal Reasons",
    500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway",
    503: "Service Unavailable", 504: "Gateway Timeout", 505: "HTTP Version Not Supported",
    506: "Variant Also Negotiates", 507: "Insufficient Storage", 508: "Loop Detected",
    510: "Not Extended", 511: "Network Authentication Required",
  };

  const SCOPES = ["runtime", "environment", "collection", "global"];
  // Lowest precedence first, as rt_core::vars::VarSource.
  const RANK = { global: 0, collection: 1, environment: 2, folder: 3, request: 4, runtime: 5 };

  // ---------------------------------------------------------------------------------------
  // Per-phase state (reset by `begin`; each phase also starts from the snapshot).

  let phase = "before-request";
  let origin = "request";
  let logs = [];
  let logText = 0;
  let logsDropped = 0;
  let tests = [];
  let varChanges = [];
  let flow = { kind: "continue" };
  let layers = [];
  let request = null;
  let initialRequest = "";
  let sends = [];
  let timers = [];
  let timerSeq = 0;
  let scriptPromises = [];
  const modules = new Map();
  let moduleCache = new Map();
  // The script debugger (js/runtime/50-debug.js): a factory registered at init, used only by a
  // run message with `debug`.
  let debugFactory = null;

  // ---------------------------------------------------------------------------------------
  // Formatting (console, errors)

  function describe(value, seen) {
    switch (typeof value) {
      case "string":
        return value;
      case "undefined":
        return "undefined";
      case "bigint":
        return `${value}n`;
      case "symbol":
        return value.toString();
      case "function":
        return value.name ? `[Function: ${value.name}]` : "[Function (anonymous)]";
      case "object":
        if (value === null) return "null";
        if (value instanceof Error) return errorText(value);
        return toJson(value, seen);
      default:
        return String(value);
    }
  }

  // Objects as JSON, like the browsers' console text: cycles as "[Circular]", functions and
  // undefined left out as JSON does.
  function toJson(value) {
    const stack = [];
    try {
      const text = json.stringify(value, function (key, v) {
        if (typeof v === "bigint") return `${v}n`;
        if (typeof v === "object" && v !== null) {
          while (stack.length && stack[stack.length - 1] !== this) stack.pop();
          if (stack.includes(v)) return "[Circular]";
          stack.push(v);
          if (v instanceof Map) return Object.fromEntries(v);
          if (v instanceof Set) return [...v];
          if (v instanceof Error) return errorText(v);
        }
        return v;
      });
      return text === undefined ? String(value) : text;
    } catch {
      return String(value);
    }
  }

  function format(args) {
    if (args.length === 0) return "";
    let rest = args;
    let head = "";
    if (typeof args[0] === "string" && args[0].includes("%")) {
      let i = 1;
      head = args[0].replace(/%([sdifoOjc%])/g, (m, c) => {
        if (c === "%") return "%";
        if (i >= args.length) return m;
        const v = args[i++];
        switch (c) {
          case "s":
            return typeof v === "string" ? v : describe(v);
          case "d":
          case "i":
            return typeof v === "object" ? "NaN" : String(c === "i" ? Math.trunc(Number(v)) : Number(v));
          case "f":
            return String(Number(v));
          case "c":
            return "";
          default:
            return describe(v);
        }
      });
      rest = args.slice(i);
      return [head, ...rest.map((a) => describe(a))].join(" ");
    }
    return rest.map((a) => describe(a)).join(" ");
  }

  function errorText(e) {
    if (e instanceof Error || (e && typeof e === "object" && "message" in e)) {
      const name = e.name || "Error";
      return e.message === "" || e.message === undefined ? String(name) : `${name}: ${e.message}`;
    }
    return describe(e);
  }

  // Where an uncaught error happened: the first stack frame in a script, e.g. "request:3".
  function location(e) {
    const stack = e && typeof e === "object" && typeof e.stack === "string" ? e.stack : "";
    for (const line of stack.split("\n")) {
      const m = /\(?((?:collection|request|folder:[^:()]*)):(\d+)(?::\d+)?\)?\s*$/.exec(line.trim());
      if (m) return `${m[1]}, line ${m[2]}`;
    }
    return "";
  }

  function uncaught(e) {
    const where = location(e);
    return where ? `${errorText(e)} (${where})` : errorText(e);
  }

  // A failed test's message: the assertion text; other errors keep their name.
  function testError(e) {
    if (e instanceof Error) {
      const plain = e.name === "Error" || e.name === "AssertionError";
      return plain ? String(e.message) : errorText(e);
    }
    return describe(e);
  }

  function log(level, args) {
    if (logs.length >= MAX_LOGS || logText >= MAX_LOG_TEXT) {
      logsDropped++;
      return;
    }
    let text = format(args);
    if (logText + text.length > MAX_LOG_TEXT) text = text.slice(0, MAX_LOG_TEXT - logText);
    logText += text.length;
    logs.push({ level, text, origin });
  }

  // ---------------------------------------------------------------------------------------
  // Headers, request, response

  function lower(name) {
    return String(name).toLowerCase();
  }

  function headersOf(list, writable, what) {
    const readOnly = () => {
      throw new TypeError(`${what} is read-only`);
    };
    const h = {
      get(name) {
        const n = lower(name);
        const e = list.find(([k]) => k.toLowerCase() === n);
        return e ? e[1] : undefined;
      },
      has(name) {
        const n = lower(name);
        return list.some(([k]) => k.toLowerCase() === n);
      },
      entries() {
        return list.map(([k, v]) => [k, v]);
      },
      toObject() {
        const o = {};
        for (const [k, v] of list) o[k.toLowerCase()] = v;
        return o;
      },
    };
    if (writable === null) return Object.freeze(h);
    h.set = function (name, value) {
      if (!writable) readOnly();
      const n = lower(name);
      const at = list.findIndex(([k]) => k.toLowerCase() === n);
      const entry = [String(name), String(value)];
      const rest = list.filter(([k]) => k.toLowerCase() !== n);
      list.length = 0;
      list.push(...rest);
      list.splice(at < 0 ? list.length : Math.min(at, list.length), 0, entry);
    };
    h.add = function (name, value) {
      if (!writable) readOnly();
      list.push([String(name), String(value)]);
    };
    h.delete = function (name) {
      if (!writable) readOnly();
      const n = lower(name);
      const rest = list.filter(([k]) => k.toLowerCase() !== n);
      list.length = 0;
      list.push(...rest);
    };
    return Object.freeze(h);
  }

  function bodyText(value) {
    if (value === undefined || value === null) return undefined;
    if (typeof value === "string") return value;
    if (value instanceof Uint8Array || value instanceof ArrayBuffer) {
      return new TextDecoder().decode(value);
    }
    return json.stringify(value);
  }

  function makeRequest(data, writable) {
    const state = {
      method: String(data.method),
      url: String(data.url),
      headers: data.headers.map(([k, v]) => [String(k), String(v)]),
      body: data.body === null || data.body === undefined ? undefined : String(data.body),
    };
    const what = "rt.request after the request was sent";
    const guard = () => {
      if (!writable) throw new TypeError(`${what} is read-only`);
    };
    const headers = headersOf(state.headers, writable, writable ? "" : what);
    const r = {
      get method() {
        return state.method;
      },
      set method(v) {
        guard();
        state.method = String(v);
      },
      get url() {
        return state.url;
      },
      set url(v) {
        guard();
        state.url = String(v);
      },
      get headers() {
        return headers;
      },
      get body() {
        return state.body;
      },
      set body(v) {
        guard();
        state.body = bodyText(v);
      },
      json() {
        return json.parse(state.body === undefined ? "" : state.body);
      },
    };
    return { api: Object.freeze(r), state };
  }

  function wireRequest(state) {
    return {
      method: state.method,
      url: state.url,
      headers: state.headers.map(([k, v]) => [k, v]),
      body: state.body === undefined ? null : state.body,
    };
  }

  function makeResponse(data) {
    const headers = headersOf(
      data.headers.map(([k, v]) => [String(k), String(v)]),
      null,
      "rt.response.headers",
    );
    let parsed;
    let parsedOk = false;
    const r = {
      status: data.status,
      statusText: STATUS_TEXT[data.status] || "",
      headers,
      text() {
        return data.body;
      },
      json() {
        if (!parsedOk) {
          parsed = json.parse(data.body);
          parsedOk = true;
        }
        return parsed;
      },
      time: data.time_ms,
      size: data.size,
      url: data.url,
    };
    return Object.freeze(r);
  }

  // ---------------------------------------------------------------------------------------
  // Variables

  function layerFor(scope) {
    let at = layers.findIndex((l) => l.layer === scope);
    if (at >= 0) return layers[at];
    const layer = { layer: scope, vars: new Map() };
    at = layers.findIndex((l) => RANK[l.layer] > RANK[scope]);
    layers.splice(at < 0 ? layers.length : at, 0, layer);
    return layer;
  }

  function lookup(name) {
    for (let i = layers.length - 1; i >= 0; i--) {
      const v = layers[i].vars.get(name);
      if (v !== undefined) return v;
    }
    return undefined;
  }

  function scopeOf(scope) {
    if (scope === undefined || scope === null) return "runtime";
    const s = String(scope);
    if (!SCOPES.includes(s)) {
      throw new TypeError(`unknown variable scope "${s}" (use ${SCOPES.join(", ")})`);
    }
    return s;
  }

  function varText(value) {
    if (typeof value === "string") return value;
    const text = json.stringify(value);
    return text === undefined ? String(value) : text;
  }

  // `{{name}}` as rt_core::vars: up to the first `}`, recursive, cycles stay verbatim.
  function replace(template, stack) {
    return String(template).replace(/\{\{([^}]+)\}\}/g, (whole, name) => {
      if (stack.includes(name)) return whole;
      const v = lookup(name);
      if (v === undefined) return whole;
      return replace(v, [...stack, name]);
    });
  }

  const vars = Object.freeze({
    get(name) {
      return lookup(String(name));
    },
    has(name) {
      return lookup(String(name)) !== undefined;
    },
    set(name, value, scope) {
      const s = scopeOf(scope);
      const n = String(name);
      const v = varText(value);
      layerFor(s).vars.set(n, v);
      varChanges.push({ name: n, value: v, scope: s });
    },
    unset(name, scope) {
      const s = scopeOf(scope);
      const n = String(name);
      layerFor(s).vars.delete(n);
      varChanges.push({ name: n, value: null, scope: s });
    },
    replace(template) {
      return replace(template, []);
    },
    toObject() {
      const o = {};
      for (const layer of layers) for (const [k, v] of layer.vars) o[k] = v;
      return o;
    },
  });

  // ---------------------------------------------------------------------------------------
  // Async: sends and timers (macrotasks), run by the engine's loop through `tick`.

  function sendRequest(req) {
    return new Promise((resolve, reject) => {
      if (req === null || typeof req !== "object") {
        throw new TypeError("rt.sendRequest needs an object with at least a url");
      }
      if (req.url === undefined || req.url === null || req.url === "") {
        throw new TypeError("rt.sendRequest needs a url");
      }
      let headers = [];
      if (Array.isArray(req.headers)) {
        headers = req.headers.map(([k, v]) => [String(k), String(v)]);
      } else if (req.headers && typeof req.headers === "object") {
        headers = Object.entries(req.headers).map(([k, v]) => [String(k), String(v)]);
      }
      let body = null;
      if (req.body !== undefined && req.body !== null) {
        if (typeof req.body === "string") {
          body = req.body;
        } else {
          body = json.stringify(req.body);
          if (!headers.some(([k]) => k.toLowerCase() === "content-type")) {
            headers.push(["Content-Type", "application/json"]);
          }
        }
      }
      const request = {
        method: req.method === undefined ? "GET" : String(req.method).toUpperCase(),
        url: String(req.url),
        headers,
        body,
      };
      sends.push({ request, resolve, reject });
    });
  }

  function sleep(ms) {
    const wait = Math.max(0, Number(ms) || 0);
    return new Promise((resolve) => {
      timers.push({ due: now() + wait, seq: timerSeq++, resolve });
    });
  }

  function call(message) {
    return json.parse(host(json.stringify(message)));
  }

  // 0: nothing left, 1: did something, 2: stop (time limit reached while waiting).
  function tick() {
    if (sends.length) {
      const s = sends.shift();
      const reply = call({ op: "send", request: s.request });
      if (reply.stop) return 2;
      if (reply.error !== undefined) s.reject(new Error(reply.error));
      else s.resolve(makeResponse(reply.response));
      return 1;
    }
    if (timers.length) {
      let next = 0;
      for (let i = 1; i < timers.length; i++) {
        const a = timers[i];
        const b = timers[next];
        if (a.due < b.due || (a.due === b.due && a.seq < b.seq)) next = i;
      }
      const t = timers.splice(next, 1)[0];
      const wait = t.due - now();
      if (wait > 0) {
        const reply = call({ op: "sleep", ms: wait });
        if (reply.stop) return 2;
      }
      t.resolve();
      return 1;
    }
    return 0;
  }

  // ---------------------------------------------------------------------------------------
  // Globals of a phase

  function test(name, fn) {
    const entry = { name: String(name), passed: false, origin };
    tests.push(entry);
    if (typeof fn !== "function") {
      entry.error = "test() needs a function";
      return;
    }
    let result;
    try {
      result = fn();
    } catch (e) {
      entry.error = testError(e);
      return;
    }
    if (result && typeof result.then === "function") {
      entry.pending = true;
      Promise.resolve(result).then(
        () => {
          entry.pending = false;
          entry.passed = true;
        },
        (e) => {
          entry.pending = false;
          entry.error = testError(e);
        },
      );
    } else {
      entry.passed = true;
    }
  }

  function require(name) {
    const n = String(name);
    if (moduleCache.has(n)) return moduleCache.get(n).exports;
    const factory = modules.get(n);
    if (!factory) throw new Error(`module "${n}" is not available in RequestTailor scripts`);
    const module = { exports: {} };
    moduleCache.set(n, module);
    factory.call(module.exports, module.exports, require, module, `${n}.js`, "");
    return module.exports;
  }

  const console = Object.freeze({
    log: (...a) => log("log", a),
    info: (...a) => log("info", a),
    warn: (...a) => log("warn", a),
    error: (...a) => log("error", a),
    debug: (...a) => log("debug", a),
    trace: (...a) => log("debug", a),
  });

  function seedRandom(seed) {
    // xorshift128+ from the host's seed: the snapshot would otherwise repeat Math.random.
    let s0 = (seed[0] >>> 0) || 0x9e3779b9;
    let s1 = (seed[1] >>> 0) || 0x243f6a88;
    let s2 = (seed[2] >>> 0) || 0xb7e15162;
    let s3 = (seed[3] >>> 0) || 0x8aed2a6b;
    Math.random = function random() {
      // xoshiro128** on 32-bit words.
      const result = Math.imul(rotl(Math.imul(s1, 5), 7), 9) >>> 0;
      const t = s1 << 9;
      s2 ^= s0;
      s3 ^= s1;
      s1 ^= s2;
      s0 ^= s3;
      s2 ^= t;
      s3 = rotl(s3, 11);
      return result / 4294967296;
    };
    function rotl(x, k) {
      return (x << k) | (x >>> (32 - k));
    }
  }

  function define(name, value) {
    Object.defineProperty(globalThis, name, {
      value,
      writable: false,
      enumerable: false,
      configurable: false,
    });
  }

  globalThis.__rt_internal = {
    addModule(name, factory) {
      modules.set(name, factory);
    },

    debugHook(factory) {
      debugFactory = factory;
    },

    begin(input) {
      phase = input.phase;
      origin = input.scripts.length ? input.scripts[0].origin : "request";
      logs = [];
      logText = 0;
      logsDropped = 0;
      tests = [];
      varChanges = [];
      flow = { kind: "continue" };
      sends = [];
      timers = [];
      scriptPromises = [];
      moduleCache = new Map();
      seedRandom(input.seed || []);
      void realRandom;

      layers = [];
      for (const v of input.variables) {
        const layer = v.source && v.source.layer ? v.source.layer : "runtime";
        let last = layers[layers.length - 1];
        const key = layer === "folder" ? `folder:${v.source.path}` : layer;
        if (!last || last.key !== key) {
          last = { layer, key, vars: new Map() };
          layers.push(last);
        }
        last.vars.set(String(v.name), String(v.value));
      }

      const writable = phase === "before-request";
      const req = makeRequest(input.request, writable);
      request = req.state;
      initialRequest = json.stringify(wireRequest(request));
      const response = input.response ? makeResponse(input.response) : undefined;

      const info = input.info || {};
      const iteration = info.iteration || null;
      const iterationData = {};
      if (iteration && Array.isArray(iteration.data)) {
        for (const [k, v] of iteration.data) iterationData[k] = v;
      }
      const rtInfo = Object.freeze({
        requestName: info.request_name || "",
        requestPath: info.request_path || "",
        collectionName: info.collection_name || "",
        environment: info.environment === null ? undefined : info.environment,
        phase,
        iteration: iteration ? iteration.index : 0,
        iterationCount: iteration ? iteration.count : 1,
        iterationData: Object.freeze(iterationData),
      });

      const runner = Object.freeze({
        skip() {
          flow = { kind: "skip" };
        },
        setNext(name) {
          flow = { kind: "next", request: name === null || name === undefined ? null : String(name) };
        },
      });

      const rt = Object.freeze({
        request: req.api,
        response,
        vars,
        info: rtInfo,
        sendRequest,
        sleep,
        runner,
      });
      define("rt", rt);
      define("test", test);
      define("console", console);
      define("require", require);
      if (input.debug && debugFactory) debugFactory(input.debug, { call, define, logs: () => logs });
    },

    enter(name) {
      origin = String(name);
    },

    scriptPromise(p) {
      scriptPromises.push(p);
    },

    tick,

    finish(error, stopped, rejections) {
      for (const [promise, reason] of rejections) {
        if (!scriptPromises.includes(promise)) log("error", [`Uncaught (in promise) ${errorText(reason)}`]);
      }
      if (logsDropped) {
        logs.push({ level: "warn", text: `${logsDropped} more log entries were dropped`, origin });
      }
      const changed = json.stringify(wireRequest(request));
      const out = {
        request: phase === "before-request" && changed !== initialRequest ? wireRequest(request) : null,
        var_changes: varChanges,
        tests: tests.map((t) => {
          const r = { name: t.name, passed: t.passed && !t.pending, origin: t.origin };
          if (t.pending) r.error = "the test did not finish";
          else if (t.error !== undefined) r.error = t.error;
          return r;
        }),
        logs,
        flow,
        error: error === undefined ? null : uncaught(error),
        stopped: stopped || null,
      };
      return json.stringify(out);
    },
  };
})();
