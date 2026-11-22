// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Bruno: `bru.*`, `req`, `res` on top of `rt` (api.d.ts). Semantics follow Bruno's safe-mode
// sandbox; differences are listed in SHIM-COVERAGE.md.
//
// Variable scopes: `bru.setVar` → runtime, `bru.setEnvVar` → environment, `bru.setGlobalEnvVar`
// → global, `bru.setCollectionVar` → collection. Folder and request variables are read as the
// resolved value. Files are never written (`persist` is ignored).

(function () {
  "use strict";
  const S = globalThis.__rtShim;
  const { define, vars, unsupported, RESPONSE, responseAdapter, parseJson, reasonOf } = S;

  function lowerHeaders(entries) {
    const out = {};
    for (const [n, v] of entries) out[n.toLowerCase()] = v;
    return out;
  }

  /** A body as Bruno hands it to scripts: parsed JSON when it parses, the text otherwise. */
  function bodyValue(text) {
    if (text === undefined || text === null) return undefined;
    const p = parseJson(text);
    return p.ok ? p.value : text;
  }

  /** `res('a.b[0].c')`: Bruno's response query (dot and bracket paths; `..key` searches
   *  deep; a key applied to an array maps over its items). */
  function query(data, path) {
    if (path === undefined || path === null || path === "") return data;
    const tokens = [];
    String(path).replace(/\.\.([^.[\]]+)|\[(-?\d+)\]|\[['"]([^'"]*)['"]\]|([^.[\]]+)/g, (m, deep, idx, q, key) => {
      if (deep !== undefined) tokens.push({ deep });
      else if (idx !== undefined) tokens.push({ index: Number(idx) });
      else tokens.push({ key: q !== undefined ? q : key });
      return m;
    });
    let cur = [data];
    let mapped = false;
    for (const t of tokens) {
      const next = [];
      for (const v of cur) {
        if (v === null || v === undefined) continue;
        if (t.deep !== undefined) {
          const walk = (x) => {
            if (x === null || typeof x !== "object") return;
            if (!Array.isArray(x) && t.deep in x) next.push(x[t.deep]);
            for (const k of Object.keys(x)) walk(x[k]);
          };
          walk(v);
          mapped = true;
        } else if (t.index !== undefined) {
          if (Array.isArray(v)) next.push(v[t.index < 0 ? v.length + t.index : t.index]);
        } else if (Array.isArray(v) && !(t.key in v)) {
          for (const x of v) if (x !== null && x !== undefined && t.key in Object(x)) next.push(x[t.key]);
          mapped = true;
        } else if (t.key in Object(v)) next.push(v[t.key]);
      }
      cur = next;
    }
    if (mapped) return cur.length ? cur.flat() : undefined;
    return cur[0];
  }

  // ---------------------------------------------------------------------------------------
  // bru

  /** `bru.sendRequest` / axios-like response. */
  function axiosResponse(r) {
    const out = {
      status: r.status,
      statusText: reasonOf(r),
      headers: lowerHeaders(r.headers.entries()),
      data: bodyValue(r.text()),
      duration: r.time,
      url: r.url,
    };
    Object.defineProperty(out, RESPONSE, { value: responseAdapter(r) });
    return out;
  }

  function sendRequest(options, callback) {
    const o = typeof options === "string" ? { url: options } : options || {};
    let url = o.url;
    if (o.params && typeof o.params === "object") {
      const qs = Object.entries(o.params)
        .map(([k, v]) => encodeURIComponent(k) + "=" + encodeURIComponent(String(v)))
        .join("&");
      if (qs) url += (url.indexOf("?") >= 0 ? "&" : "?") + qs;
    }
    const promise = rt
      .sendRequest({ method: String(o.method || "GET").toUpperCase(), url, headers: o.headers || {}, body: o.data === undefined ? o.body : o.data })
      .then(axiosResponse);
    if (typeof callback !== "function") return promise;
    return promise.then(
      (res) => {
        callback(null, res);
        return res;
      },
      (err) => {
        callback(err, null);
      },
    );
  }

  function interpolate(value) {
    if (typeof value === "string") return vars.replace(value);
    if (Array.isArray(value)) return value.map(interpolate);
    if (value !== null && typeof value === "object") {
      const out = {};
      for (const k of Object.keys(value)) out[k] = interpolate(value[k]);
      return out;
    }
    return value;
  }

  const runner = {
    skipRequest: () => rt.runner.skip(),
    stopExecution: () => rt.runner.setNext(null),
    setNextRequest: (name) => rt.runner.setNext(name === undefined ? null : name),
  };

  const bru = {
    // Runtime variables
    getVar: (name) => vars.get(name, "runtime"),
    setVar(name, value) {
      if (!/^[\w-.]+$/.test(String(name))) throw new Error("Variable name: \"" + name + "\" contains invalid characters! Names must only contain alpha-numeric characters, \"-\", \"_\", \".\"");
      vars.set(name, value, "runtime");
    },
    hasVar: (name) => vars.has(name, "runtime"),
    deleteVar: (name) => vars.unset(name, "runtime"),
    deleteAllVars: unsupported("bru.deleteAllVars", "the runtime variables cannot be listed"),
    getAllVars: () => vars.toObject(),
    // Environment
    getEnvName: () => rt.info.environment,
    getEnvVar: (name) => vars.get(name, "environment"),
    setEnvVar: (name, value) => vars.set(name, value, "environment"),
    hasEnvVar: (name) => vars.has(name, "environment"),
    deleteEnvVar: (name) => vars.unset(name, "environment"),
    // Global environment
    getGlobalEnvVar: (name) => vars.get(name, "global"),
    setGlobalEnvVar: (name, value) => vars.set(name, value, "global"),
    // Collection, folder, request variables (folder and request: the resolved value)
    getCollectionVar: (name) => vars.get(name, "collection"),
    setCollectionVar: (name, value) => vars.set(name, value, "collection"),
    hasCollectionVar: (name) => vars.has(name, "collection"),
    deleteCollectionVar: (name) => vars.unset(name, "collection"),
    getFolderVar: (name) => vars.resolved(name),
    getRequestVar: (name) => vars.resolved(name),
    getCollectionName: () => rt.info.collectionName,
    getProcessEnv: () => undefined,
    getOauth2CredentialVar: unsupported("bru.getOauth2CredentialVar"),
    interpolate,
    sleep: (ms) => rt.sleep(ms),
    setNextRequest: (name) => rt.runner.setNext(name === undefined ? null : name),
    runner,
    sendRequest,
    runRequest: unsupported("bru.runRequest", "use the collection runner"),
    isSafeMode: () => true,
    cwd: unsupported("bru.cwd", "scripts have no file system access"),
    getTestResults: unsupported("bru.getTestResults"),
    getAssertionResults: unsupported("bru.getAssertionResults"),
    visualize() {
      console.warn("bru.visualize is not supported in RequestTailor; ignored");
    },
    cookies: { jar: unsupported("bru.cookies.jar") },
  };
  define("bru", bru);

  // ---------------------------------------------------------------------------------------
  // req

  const r = rt.request;
  const req = {
    getUrl: () => r.url,
    setUrl(url) {
      r.url = String(url);
    },
    getMethod: () => r.method,
    setMethod(method) {
      r.method = String(method).toUpperCase();
    },
    getHeader: (name) => r.headers.get(name),
    getHeaders() {
      const out = {};
      for (const [n, v] of r.headers.entries()) out[n] = v;
      return out;
    },
    setHeader(name, value) {
      r.headers.set(String(name), String(value));
    },
    setHeaders(headers) {
      for (const [n] of r.headers.entries()) r.headers.delete(n);
      for (const k of Object.keys(headers || {})) r.headers.add(k, String(headers[k]));
    },
    deleteHeader(name) {
      r.headers.delete(String(name));
    },
    deleteHeaders(names) {
      for (const n of names || []) r.headers.delete(String(n));
    },
    getBody(options) {
      if (options && options.raw) return r.body;
      return bodyValue(r.body);
    },
    setBody(data) {
      r.body = typeof data === "string" ? data : JSON.stringify(data);
    },
    getName: () => rt.info.requestName,
    getPathParams: unsupported("req.getPathParams"),
    getAuthMode: unsupported("req.getAuthMode"),
    getTimeout: unsupported("req.getTimeout"),
    setTimeout: unsupported("req.setTimeout"),
    setMaxRedirects: unsupported("req.setMaxRedirects"),
    getExecutionMode: unsupported("req.getExecutionMode"),
    getExecutionPlatform: () => "app",
    onFail: unsupported("req.onFail"),
  };
  Object.defineProperties(req, {
    url: { get: () => r.url, set: (v) => req.setUrl(v), enumerable: true },
    method: { get: () => r.method, set: (v) => req.setMethod(v), enumerable: true },
    headers: { get: () => req.getHeaders(), enumerable: true },
    body: { get: () => req.getBody(), set: (v) => req.setBody(v), enumerable: true },
  });
  define("req", req);

  // ---------------------------------------------------------------------------------------
  // res (after the response only)

  const response = rt.response;
  if (response) {
    let body = bodyValue(response.text());
    const res = function (path) {
      return query(body, path);
    };
    Object.assign(res, {
      getStatus: () => response.status,
      getStatusText: () => reasonOf(response),
      getHeader: (name) => response.headers.get(name),
      getHeaders: () => lowerHeaders(response.headers.entries()),
      getBody: () => body,
      setBody(value) {
        body = value;
      },
      getResponseTime: () => response.time,
      getUrl: () => response.url,
      getSize: () => {
        let header = 0;
        for (const [n, v] of response.headers.entries()) header += n.length + v.length + 4;
        return { header, body: response.size, total: header + response.size };
      },
    });
    Object.defineProperties(res, {
      status: { get: () => response.status, enumerable: true },
      statusText: { get: () => reasonOf(response), enumerable: true },
      headers: { get: () => res.getHeaders(), enumerable: true },
      body: { get: () => body, enumerable: true },
      responseTime: { get: () => response.time, enumerable: true },
      url: { get: () => response.url, enumerable: true },
      [RESPONSE]: { value: responseAdapter(response) },
    });
    define("res", res);
  }

  S.bruQuery = query;
})();
