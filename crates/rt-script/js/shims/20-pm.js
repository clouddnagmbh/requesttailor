// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Postman: `pm.*`, `postman.*` and the legacy sandbox globals (`tests[...]`, `responseBody`,
// `responseCode`, `responseHeaders`, `responseTime`, `environment`, `globals`, `iteration`,
// `request`, `_`, `CryptoJS`, `tv4`), on top of `rt` (api.d.ts). Insomnia's `insomnia.*`
// is the same API under another name (with `baseEnvironment` and `baseGlobals`).
//
// Scopes: `pm.environment` → environment, `pm.collectionVariables` → collection,
// `pm.globals` → global, `pm.variables.set` → runtime (Postman's local variables);
// `pm.variables.get` reads the resolved value (all layers), `pm.iterationData` the runner's
// data row. Files are never written.

(function () {
  "use strict";
  const G = globalThis;
  const S = G.__rtShim;
  const { define, vars, unsupported, RESPONSE, responseAdapter, parseJson, reasonOf, formEncode } = S;

  const phase = rt.info.phase;
  const before = phase === "before-request";

  // ---------------------------------------------------------------------------------------
  // Lists (postman-collection's PropertyList, the parts scripts use)

  /** A list of `{ key, value }` members; `onChange` runs after every change. */
  function propertyList(read, write) {
    const lower = (s) => String(s).toLowerCase();
    const find = (key) => read().find((m) => lower(m.key) === lower(key));
    const list = {
      get: (key) => (find(key) || {}).value,
      one: (key) => find(key),
      has(key, value) {
        if (typeof key === "object" && key !== null) return read().some((m) => lower(m.key) === lower(key.key));
        const m = read().filter((x) => lower(x.key) === lower(key));
        return arguments.length > 1 ? m.some((x) => x.value === value) : m.length > 0;
      },
      all: () => read().map((m) => ({ key: m.key, value: m.value })),
      members: undefined,
      count: () => read().length,
      idx: (i) => read()[i],
      each(fn) {
        read().forEach((m) => fn(m));
      },
      map: (fn) => read().map(fn),
      filter: (fn) => read().filter(fn),
      find: (fn) => read().find(fn),
      reduce: (fn, acc) => read().reduce(fn, acc),
      indexOf: (key) => read().findIndex((m) => lower(m.key) === lower(typeof key === "object" ? key.key : key)),
      toObject(excludeDisabled, caseSensitive) {
        const out = {};
        for (const m of read()) out[caseSensitive === false ? lower(m.key) : m.key] = m.value;
        return out;
      },
      toString: () => read().map((m) => m.key + ": " + m.value).join("\n"),
      toJSON: () => list.all(),
    };
    Object.defineProperty(list, "members", { get: () => list.all(), enumerable: true });
    if (write) {
      const member = (item, value) => {
        if (typeof item === "string") {
          if (value !== undefined) return { key: item, value: String(value) };
          const i = item.indexOf(":");
          return i < 0 ? { key: item.trim(), value: "" } : { key: item.slice(0, i).trim(), value: item.slice(i + 1).trim() };
        }
        return { key: String(item.key), value: item.value === undefined || item.value === null ? "" : String(item.value) };
      };
      Object.assign(list, {
        add(item, value) {
          write("add", member(item, value));
        },
        append(item, value) {
          write("add", member(item, value));
        },
        upsert(item, value) {
          write("upsert", member(item, value));
        },
        remove(predicate) {
          const keys = read()
            .filter((m) => (typeof predicate === "function" ? predicate(m) : lower(m.key) === lower(typeof predicate === "object" ? predicate.key : predicate)))
            .map((m) => m.key);
          for (const k of keys) write("remove", { key: k });
        },
        clear() {
          for (const m of read()) write("remove", { key: m.key });
        },
        populate(items) {
          for (const i of items || []) write("add", member(i));
        },
        repopulate(items) {
          list.clear();
          list.populate(items);
        },
      });
    }
    return list;
  }

  // ---------------------------------------------------------------------------------------
  // Variable scopes

  function scope(name, rtScope, extra) {
    const s = {
      get: (key) => vars.get(key, rtScope),
      set(key, value) {
        if (arguments.length < 2) throw new Error("pm." + name + ".set needs a key and a value");
        vars.set(key, value, rtScope);
      },
      unset: (key) => vars.unset(key, rtScope),
      has: (key) => vars.has(key, rtScope),
      clear: unsupported("pm." + name + ".clear", "the variables of one scope cannot be listed"),
      toObject: () => vars.toObject(),
      toJSON: () => ({ values: Object.entries(vars.toObject()).map(([key, value]) => ({ key, value })) }),
      replaceIn: (template) => vars.replace(template),
    };
    return Object.assign(s, extra || {});
  }

  const environment = scope("environment", "environment");
  Object.defineProperty(environment, "name", { get: () => rt.info.environment, enumerable: true });
  const globals = scope("globals", "global");
  const collectionVariables = scope("collectionVariables", "collection");
  const variables = {
    get: (key) => vars.resolved(key),
    set: (key, value) => vars.set(key, value, "runtime"),
    has: (key) => vars.resolved(key) !== undefined,
    unset: (key) => vars.unset(key, "runtime"),
    toObject: () => vars.toObject(),
    replaceIn: (template) => vars.replace(template),
  };
  const data = Object.assign({}, rt.info.iterationData);
  const iterationData = {
    get: (key) => data[key],
    has: (key) => Object.prototype.hasOwnProperty.call(data, key),
    unset: (key) => void delete data[key],
    toObject: () => Object.assign({}, data),
    toJSON: () => ({ values: Object.entries(data).map(([key, value]) => ({ key, value })) }),
    replaceIn: (template) => String(template).replace(/\{\{([^{}]+)\}\}/g, (m, k) => (k.trim() in data ? String(data[k.trim()]) : m)),
  };

  // ---------------------------------------------------------------------------------------
  // Request

  const r = rt.request;

  /** Splits a URL as written (templates allowed) into its parts. */
  function splitUrl(raw) {
    const m = /^(?:([A-Za-z][\w+.-]*):\/\/)?(?:([^@/?#]*)@)?([^/?#]*)([^?#]*)(?:\?([^#]*))?(?:#(.*))?$/.exec(raw) || [];
    const auth = m[2];
    let host = m[3] || "";
    let port;
    const pm = /^(.*?):(\d+|\{\{[^}]+\}\})$/.exec(host);
    if (pm && !host.startsWith("[")) {
      host = pm[1];
      port = pm[2];
    }
    return {
      protocol: m[1],
      auth: auth === undefined ? undefined : { user: decodeURIComponent(auth.split(":")[0]), password: auth.indexOf(":") >= 0 ? decodeURIComponent(auth.slice(auth.indexOf(":") + 1)) : undefined },
      host,
      port,
      path: m[4] || "",
      query: m[5],
      hash: m[6],
    };
  }

  function parseQuery(q) {
    if (q === undefined || q === "") return [];
    return q.split("&").map((p) => {
      const i = p.indexOf("=");
      return i < 0 ? { key: p, value: null } : { key: p.slice(0, i), value: p.slice(i + 1) };
    });
  }

  function joinUrl(parts, query) {
    let s = "";
    if (parts.protocol) s += parts.protocol + "://";
    if (parts.auth && parts.auth.user !== undefined) s += encodeURIComponent(parts.auth.user) + (parts.auth.password !== undefined ? ":" + encodeURIComponent(parts.auth.password) : "") + "@";
    s += parts.host;
    if (parts.port !== undefined) s += ":" + parts.port;
    s += parts.path;
    if (query.length) s += "?" + query.map((q) => (q.value === null || q.value === undefined ? q.key : q.key + "=" + q.value)).join("&");
    if (parts.hash !== undefined) s += "#" + parts.hash;
    return s;
  }

  /** postman-collection's `Url` over a getter/setter of the raw URL. */
  function url(get, set) {
    const parts = () => splitUrl(get());
    const query = propertyList(
      () => parseQuery(parts().query),
      set &&
        ((op, m) => {
          const p = parts();
          let q = parseQuery(p.query);
          const enc = (v) => String(v).replace(/[&#]/g, encodeURIComponent);
          if (op === "remove") q = q.filter((x) => x.key !== m.key);
          else if (op === "upsert" && q.some((x) => x.key === m.key)) q = q.map((x) => (x.key === m.key ? { key: x.key, value: enc(m.value) } : x));
          else q.push({ key: enc(m.key), value: enc(m.value) });
          set(joinUrl(p, q));
        }),
    );
    const u = {
      toString: () => get(),
      getRaw: () => get(),
      getHost: () => parts().host,
      getRemote: () => parts().host + (parts().port !== undefined ? ":" + parts().port : ""),
      getPath: () => parts().path || "/",
      getPathWithQuery: () => (parts().path || "/") + (parts().query !== undefined && parts().query !== "" ? "?" + parts().query : ""),
      getQueryString: () => parts().query || "",
      getOAuth1BaseUrl: unsupported("pm.request.url.getOAuth1BaseUrl"),
      query,
      variables: propertyList(() => []),
      addQueryParams(params) {
        if (typeof params === "string") params = parseQuery(params);
        for (const p of [].concat(params)) query.add(p);
      },
      removeQueryParams(names) {
        for (const n of [].concat(names)) query.remove(typeof n === "object" ? n.key : n);
      },
      update(v) {
        if (!set) throw new TypeError("the URL is read-only here");
        set(String(v));
      },
      toJSON: () => get(),
    };
    Object.defineProperties(u, {
      protocol: { get: () => parts().protocol, enumerable: true },
      host: { get: () => (parts().host ? parts().host.split(".") : []), enumerable: true },
      port: { get: () => parts().port, enumerable: true },
      path: { get: () => parts().path.split("/").filter((x) => x !== ""), enumerable: true },
      hash: { get: () => parts().hash, enumerable: true },
      auth: { get: () => parts().auth, enumerable: true },
    });
    return u;
  }

  const headerList = (headers, writable) =>
    propertyList(
      () => headers.entries().map(([key, value]) => ({ key, value })),
      writable &&
        ((op, m) => {
          if (op === "remove") headers.delete(m.key);
          else if (op === "upsert") headers.set(m.key, m.value);
          else headers.add(m.key, m.value);
        }),
    );

  const requestHeaders = headerList(r.headers, true);
  const requestUrl = url(
    () => r.url,
    (v) => {
      r.url = v;
    },
  );

  function bodyUpdate(value) {
    if (value === undefined || value === null) return;
    if (typeof value === "string") r.body = value;
    else if (value.mode === "raw" || "raw" in value) r.body = typeof value.raw === "string" ? value.raw : JSON.stringify(value.raw);
    else if (value.mode === "urlencoded") r.body = formEncode((value.urlencoded || []).filter((p) => !p.disabled).map((p) => [p.key, p.value]));
    else if (value.mode === "graphql") r.body = JSON.stringify(value.graphql || {});
    else throw new Error("pm.request.body.update: body mode '" + value.mode + "' is not supported in RequestTailor; see SHIM-COVERAGE.md");
  }

  const requestBody = {
    update: bodyUpdate,
    isEmpty: () => r.body === undefined || r.body === "",
    toString: () => (r.body === undefined ? "" : r.body),
    toJSON: () => (r.body === undefined ? {} : { mode: "raw", raw: r.body }),
  };
  Object.defineProperties(requestBody, {
    mode: { get: () => (r.body === undefined ? undefined : "raw"), enumerable: true },
    raw: { get: () => r.body, set: (v) => bodyUpdate(String(v)), enumerable: true },
  });

  const request = {
    headers: requestHeaders,
    body: requestBody,
    addHeader: (h, v) => requestHeaders.add(h, v),
    removeHeader: (h) => requestHeaders.remove(h),
    upsertHeader: (h, v) => requestHeaders.upsert(h, v),
    getHeaders(options) {
      const out = {};
      for (const [k, v] of r.headers.entries()) out[options && options.ignoreCase ? k.toLowerCase() : k] = v;
      return out;
    },
    addQueryParams: (p) => requestUrl.addQueryParams(p),
    removeQueryParams: (p) => requestUrl.removeQueryParams(p),
    toJSON: () => ({ method: r.method, url: r.url, header: requestHeaders.all(), body: requestBody.toJSON() }),
  };
  Object.defineProperties(request, {
    url: {
      get: () => requestUrl,
      set: (v) => {
        r.url = typeof v === "string" ? v : String(v && v.raw !== undefined ? v.raw : v);
      },
      enumerable: true,
    },
    method: {
      get: () => r.method,
      set: (v) => {
        r.method = String(v).toUpperCase();
      },
      enumerable: true,
    },
    name: { get: () => rt.info.requestName, enumerable: true },
    id: { get: () => rt.info.requestPath, enumerable: true },
    auth: { get: unsupported("pm.request.auth"), enumerable: false },
  });

  // ---------------------------------------------------------------------------------------
  // Response

  /** Cookies from `Set-Cookie` headers (response) or the `Cookie` header (request). */
  function cookieList(read) {
    const list = propertyList(read);
    list.jar = unsupported("pm.cookies.jar");
    return list;
  }
  function setCookies(headers) {
    return headers
      .entries()
      .filter(([n]) => n.toLowerCase() === "set-cookie")
      .map(([, v]) => {
        const first = v.split(";")[0];
        const i = first.indexOf("=");
        return { key: first.slice(0, i).trim(), value: first.slice(i + 1).trim(), name: first.slice(0, i).trim() };
      });
  }

  function response(rr) {
    let cached;
    const res = {
      text: () => rr.text(),
      json() {
        if (!cached) cached = { value: rr.json() };
        return cached.value;
      },
      reason: () => reasonOf(rr),
      headers: headerList(rr.headers, false),
      cookies: cookieList(() => setCookies(rr.headers)),
      size: () => ({ body: rr.size, header: 0, total: rr.size }),
      toJSON: () => ({ code: rr.status, status: reasonOf(rr), header: res.headers.all(), body: rr.text(), responseTime: rr.time }),
    };
    Object.defineProperties(res, {
      code: { get: () => rr.status, enumerable: true },
      status: { get: () => reasonOf(rr), enumerable: true },
      responseTime: { get: () => rr.time, enumerable: true },
      responseSize: { get: () => rr.size, enumerable: true },
      to: { get: () => expect(res).to },
      not: { get: () => expect(res).not },
      [RESPONSE]: { value: responseAdapter(rr) },
    });
    return res;
  }

  const pmResponse = rt.response ? response(rt.response) : undefined;

  // ---------------------------------------------------------------------------------------
  // sendRequest

  function sendRequest(req, callback) {
    let out;
    try {
      out = toRtRequest(req);
    } catch (e) {
      if (typeof callback === "function") return Promise.resolve().then(() => callback(e, null));
      return Promise.reject(e);
    }
    const promise = rt.sendRequest(out).then(response);
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

  function toRtRequest(req) {
    if (typeof req === "string") return { method: "GET", url: req };
    if (!req || req.url === undefined) throw new TypeError("pm.sendRequest needs a URL or { url, method, header, body }");
    const u = typeof req.url === "string" ? req.url : req.url.raw !== undefined ? req.url.raw : String(req.url);
    let headers = [];
    const h = req.header !== undefined ? req.header : req.headers;
    if (Array.isArray(h)) headers = h.filter((x) => x && !x.disabled).map((x) => (typeof x === "string" ? x.split(/:(.*)/s).map((s) => s.trim()).slice(0, 2) : [x.key, String(x.value)]));
    else if (typeof h === "string") headers = h.split("\n").filter(Boolean).map((l) => l.split(/:(.*)/s).map((s) => s.trim()).slice(0, 2));
    else if (h && typeof h === "object") headers = Object.keys(h).map((k) => [k, String(h[k])]);
    let body;
    const b = req.body;
    const setType = (t) => {
      if (!headers.some(([n]) => n.toLowerCase() === "content-type")) headers.push(["Content-Type", t]);
    };
    if (b !== undefined && b !== null) {
      if (typeof b === "string") body = b;
      else if (b.mode === "raw" || (b.mode === undefined && "raw" in b)) body = typeof b.raw === "string" ? b.raw : JSON.stringify(b.raw);
      else if (b.mode === "urlencoded") {
        body = formEncode((b.urlencoded || []).filter((p) => !p.disabled).map((p) => [p.key, p.value]));
        setType("application/x-www-form-urlencoded");
      } else if (b.mode === "graphql") {
        body = JSON.stringify({ query: b.graphql && b.graphql.query, variables: b.graphql && b.graphql.variables });
        setType("application/json");
      } else if (b.mode) throw new Error("pm.sendRequest: body mode '" + b.mode + "' is not supported in RequestTailor; see SHIM-COVERAGE.md");
      else body = b;
    }
    return { method: String(req.method || "GET").toUpperCase(), url: u, headers, body };
  }

  // ---------------------------------------------------------------------------------------
  // pm

  function pmTest(name, fn) {
    if (typeof fn !== "function") return test(String(name), () => {});
    if (fn.length > 0) {
      // Postman's `done` callback style.
      return test(String(name), () => new Promise((resolve, reject) => {
        try {
          fn((err) => (err ? reject(err) : resolve()));
        } catch (e) {
          reject(e);
        }
      }));
    }
    return test(String(name), fn);
  }
  pmTest.skip = function () {};

  const execution = {
    skipRequest() {
      if (!before) throw new Error("pm.execution.skipRequest works in pre-request scripts only");
      rt.runner.skip();
    },
    setNextRequest: (name) => rt.runner.setNext(name === undefined ? null : name),
  };
  Object.defineProperty(execution, "location", { get: () => Object.assign([rt.info.collectionName], { current: rt.info.requestName }), enumerable: true });

  const info = {};
  Object.defineProperties(info, {
    eventName: { get: () => (before ? "prerequest" : "test"), enumerable: true },
    iteration: { get: () => rt.info.iteration, enumerable: true },
    iterationCount: { get: () => rt.info.iterationCount, enumerable: true },
    requestName: { get: () => rt.info.requestName, enumerable: true },
    requestId: { get: () => rt.info.requestPath, enumerable: true },
  });

  let warnedVisualizer = false;
  const pm = {
    environment,
    globals,
    collectionVariables,
    variables,
    iterationData,
    request,
    response: pmResponse,
    info,
    test: pmTest,
    expect: (v, m) => expect(v, m),
    sendRequest,
    execution,
    cookies: cookieList(() =>
      pmResponse
        ? setCookies(rt.response.headers)
        : (r.headers.get("cookie") || "")
            .split(";")
            .map((c) => c.trim())
            .filter(Boolean)
            .map((c) => ({ key: c.split("=")[0], value: c.slice(c.indexOf("=") + 1) })),
    ),
    visualizer: {
      set() {
        if (!warnedVisualizer) console.warn("pm.visualizer is not supported in RequestTailor; ignored");
        warnedVisualizer = true;
      },
      clear() {},
    },
    vault: { get: unsupported("pm.vault"), set: unsupported("pm.vault"), unset: unsupported("pm.vault") },
    require: unsupported("pm.require", "package libraries are not available; built-in modules work with require()"),
  };
  define("pm", pm);

  // Insomnia: the same API (`baseEnvironment` is the collection's, `baseGlobals` the global
  // one); what is Insomnia-only throws.
  const insomnia = Object.assign({}, pm, {
    baseEnvironment: scope("baseEnvironment", "collection"),
    baseGlobals: scope("baseGlobals", "global"),
    parentFolders: { get: unsupported("insomnia.parentFolders"), getByName: unsupported("insomnia.parentFolders"), findValue: unsupported("insomnia.parentFolders") },
  });
  define("insomnia", insomnia);

  // ---------------------------------------------------------------------------------------
  // postman.* and the legacy globals

  define("postman", {
    setNextRequest: (name) => rt.runner.setNext(name === undefined ? null : name),
    getEnvironmentVariable: (k) => vars.get(k, "environment"),
    setEnvironmentVariable: (k, v) => vars.set(k, v, "environment"),
    clearEnvironmentVariable: (k) => vars.unset(k, "environment"),
    clearEnvironmentVariables: unsupported("postman.clearEnvironmentVariables"),
    getGlobalVariable: (k) => vars.get(k, "global"),
    setGlobalVariable: (k, v) => vars.set(k, v, "global"),
    clearGlobalVariable: (k) => vars.unset(k, "global"),
    clearGlobalVariables: unsupported("postman.clearGlobalVariables"),
    getResponseHeader: (name) => (rt.response ? rt.response.headers.get(name) : undefined),
    getResponseCookie(name) {
      const c = rt.response && setCookies(rt.response.headers).find((x) => x.key === name);
      return c ? { name: c.key, value: c.value } : undefined;
    },
  });

  // `tests["name"] = condition` records a test.
  define(
    "tests",
    new Proxy(
      {},
      {
        set(target, name, value) {
          target[name] = value;
          test(String(name), () => {
            if (!value) throw new S.AssertionError("expected " + S.inspect(value) + " to be truthy");
          });
          return true;
        },
      },
    ),
  );

  define("iteration", rt.info.iteration);
  define("environment", vars.toObject());
  define("globals", vars.toObject());
  define("data", Object.assign({}, rt.info.iterationData));
  define("request", {
    url: r.url,
    method: r.method,
    headers: request.getHeaders(),
    data: r.body === undefined ? {} : r.body,
    name: rt.info.requestName,
    id: rt.info.requestPath,
  });
  if (rt.response) {
    const rr = rt.response;
    define("responseBody", rr.text());
    define("responseCode", { code: rr.status, name: reasonOf(rr), detail: reasonOf(rr) });
    const headers = {};
    for (const [n, v] of rr.headers.entries()) headers[n] = v;
    define("responseHeaders", headers);
    define("responseTime", rr.time);
    define("responseCookies", setCookies(rr.headers).map((c) => ({ name: c.key, value: c.value })));
  }
  define("xml2Json", unsupported("xml2Json"));
  define("xmlToJson", unsupported("xmlToJson"));
  define("cheerio", unsupported("cheerio"));

  // Lazy module globals of the Postman sandbox.
  for (const [name, module] of [["_", "lodash"], ["CryptoJS", "crypto-js"], ["tv4", "tv4"]]) {
    Object.defineProperty(G, name, {
      get() {
        const value = require(module);
        Object.defineProperty(G, name, { value, writable: true, configurable: true });
        return value;
      },
      set(value) {
        Object.defineProperty(G, name, { value, writable: true, configurable: true });
      },
      configurable: true,
      enumerable: false,
    });
  }

  S.pmResponse = response;
  S.parseJson = parseJson;
})();
