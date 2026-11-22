// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Helpers shared by the Bruno and Postman shims, and the globals both sandboxes have:
// `setTimeout`/`clearTimeout`/`setInterval`/`clearInterval` on top of `rt.sleep`, `atob`,
// `btoa`, `queueMicrotask`. Added to `__rtShim`: `vars` (typed variable access), `unsupported`,
// `responseAdapter`, `utf8Encode`/`utf8Decode`, `parseJson`.

(function () {
  "use strict";
  const G = globalThis;
  const S = G.__rtShim;
  const define = S.define;

  /** A function that throws a clear error for an API RequestTailor does not provide. */
  function unsupported(name, why) {
    return function () {
      throw new Error(name + " is not supported in RequestTailor" + (why ? " (" + why + ")" : "") + "; see SHIM-COVERAGE.md");
    };
  }

  // ---------------------------------------------------------------------------------------
  // Variables with types.
  //
  // `rt.vars` keeps strings (other values as JSON). Bruno and Postman scripts get back the
  // value they set, with its type, for the rest of the phase; values from earlier phases,
  // requests and files come back as strings. A scope-specific read sees what this phase set
  // in that scope (or unset there), otherwise the resolved value.

  /** Highest precedence first. */
  const SCOPES = ["runtime", "environment", "collection", "global"];
  const written = new Map(); // `${scope}\u0000${name}` → { value, text, deleted }

  function textOf(value) {
    if (typeof value === "string") return value;
    const json = JSON.stringify(value);
    return json === undefined ? String(value) : json;
  }

  const vars = {
    set(name, value, scope) {
      name = String(name);
      scope = scope || "runtime";
      if (value === undefined) return vars.unset(name, scope);
      rt.vars.set(name, value, scope);
      written.set(scope + "\u0000" + name, { value, text: textOf(value), scope });
    },
    unset(name, scope) {
      name = String(name);
      scope = scope || "runtime";
      rt.vars.unset(name, scope);
      written.set(scope + "\u0000" + name, { deleted: true, scope });
    },
    /** The value in `scope` as far as this phase knows it, else the resolved one. */
    get(name, scope) {
      name = String(name);
      if (scope) {
        const w = written.get(scope + "\u0000" + name);
        if (w) return w.deleted ? undefined : w.value;
      }
      return vars.resolved(name);
    },
    has(name, scope) {
      return vars.get(name, scope) !== undefined;
    },
    /** The resolved value (all layers), typed if this phase set it. */
    resolved(name) {
      name = String(name);
      const text = rt.vars.get(name);
      if (text === undefined) return undefined;
      for (const scope of SCOPES) {
        const w = written.get(scope + "\u0000" + name);
        if (w && !w.deleted && w.text === text) return w.value;
      }
      return text;
    },
    toObject() {
      const out = {};
      const all = rt.vars.toObject();
      for (const k of Object.keys(all)) out[k] = vars.resolved(k);
      return out;
    },
    replace(template) {
      return rt.vars.replace(String(template));
    },
  };

  // ---------------------------------------------------------------------------------------
  // Timers on `rt.sleep` (time counts against the script's limit)

  let nextTimer = 1;
  const timers = new Map();
  if (typeof G.setTimeout !== "function") {
    define("setTimeout", function (fn, ms) {
      const args = Array.prototype.slice.call(arguments, 2);
      const id = nextTimer++;
      timers.set(id, true);
      rt.sleep(Math.max(0, Number(ms) || 0)).then(() => {
        if (!timers.delete(id)) return;
        if (typeof fn === "function") fn.apply(undefined, args);
      });
      return id;
    });
    define("clearTimeout", function (id) {
      timers.delete(id);
    });
    define("setInterval", function (fn, ms) {
      const args = Array.prototype.slice.call(arguments, 2);
      const id = nextTimer++;
      timers.set(id, true);
      const step = () =>
        rt.sleep(Math.max(1, Number(ms) || 0)).then(() => {
          if (!timers.has(id)) return;
          fn.apply(undefined, args);
          if (timers.has(id)) return step();
        });
      step();
      return id;
    });
    define("clearInterval", function (id) {
      timers.delete(id);
    });
  }
  if (typeof G.queueMicrotask !== "function") {
    define("queueMicrotask", function (fn) {
      Promise.resolve().then(fn);
    });
  }

  // ---------------------------------------------------------------------------------------
  // Text

  function utf8Encode(str) {
    const out = [];
    for (let i = 0; i < str.length; i++) {
      let c = str.charCodeAt(i);
      if (c >= 0xd800 && c < 0xdc00 && i + 1 < str.length) {
        const d = str.charCodeAt(i + 1);
        if (d >= 0xdc00 && d < 0xe000) {
          c = 0x10000 + ((c - 0xd800) << 10) + (d - 0xdc00);
          i++;
        } else c = 0xfffd;
      } else if (c >= 0xd800 && c < 0xe000) c = 0xfffd;
      if (c < 0x80) out.push(c);
      else if (c < 0x800) out.push(0xc0 | (c >> 6), 0x80 | (c & 63));
      else if (c < 0x10000) out.push(0xe0 | (c >> 12), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
      else out.push(0xf0 | (c >> 18), 0x80 | ((c >> 12) & 63), 0x80 | ((c >> 6) & 63), 0x80 | (c & 63));
    }
    return out;
  }

  function utf8Decode(bytes) {
    let s = "";
    for (let i = 0; i < bytes.length; ) {
      const b = bytes[i++];
      let c;
      if (b < 0x80) c = b;
      else if (b >= 0xc0 && b < 0xe0) c = ((b & 31) << 6) | (bytes[i++] & 63);
      else if (b >= 0xe0 && b < 0xf0) c = ((b & 15) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
      else if (b >= 0xf0) c = ((b & 7) << 18) | ((bytes[i++] & 63) << 12) | ((bytes[i++] & 63) << 6) | (bytes[i++] & 63);
      else c = 0xfffd;
      s += String.fromCodePoint(c);
    }
    return s;
  }

  const B64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

  function btoa(input) {
    const s = String(input);
    let out = "";
    for (let i = 0; i < s.length; i += 3) {
      const a = s.charCodeAt(i);
      const b = s.charCodeAt(i + 1);
      const c = s.charCodeAt(i + 2);
      if (a > 255 || b > 255 || c > 255) throw new Error("InvalidCharacterError: btoa() takes Latin-1 text only");
      const n = (a << 16) | ((b || 0) << 8) | (c || 0);
      out += B64[n >> 18] + B64[(n >> 12) & 63] + (i + 1 < s.length ? B64[(n >> 6) & 63] : "=") + (i + 2 < s.length ? B64[n & 63] : "=");
    }
    return out;
  }

  function atob(input) {
    let s = String(input).replace(/[\t\n\f\r ]/g, "");
    if (s.length % 4 === 0) s = s.replace(/==?$/, "");
    if (s.length % 4 === 1 || /[^A-Za-z0-9+/]/.test(s)) throw new Error("InvalidCharacterError: atob() got invalid base64");
    let out = "";
    let bits = 0;
    let n = 0;
    for (const ch of s) {
      n = (n << 6) | B64.indexOf(ch);
      bits += 6;
      if (bits >= 8) {
        bits -= 8;
        out += String.fromCharCode((n >> bits) & 255);
      }
    }
    return out;
  }

  if (typeof G.btoa !== "function") define("btoa", btoa);
  if (typeof G.atob !== "function") define("atob", atob);

  function parseJson(text) {
    try {
      return { ok: true, value: JSON.parse(text) };
    } catch (e) {
      return { ok: false, error: e };
    }
  }

  /** The `__rtShim.RESPONSE` adapter of an `RtResponse`. */
  function responseAdapter(r) {
    return () => ({
      code: r.status,
      reason: r.statusText,
      header: (n) => r.headers.get(n),
      text: () => r.text(),
      json: () => r.json(),
      time: r.time,
    });
  }

  /** Reason phrase of a status code (for responses whose `statusText` is empty). */
  const REASONS = {
    100: "Continue", 101: "Switching Protocols", 200: "OK", 201: "Created", 202: "Accepted",
    203: "Non-Authoritative Information", 204: "No Content", 205: "Reset Content",
    206: "Partial Content", 300: "Multiple Choices", 301: "Moved Permanently", 302: "Found",
    303: "See Other", 304: "Not Modified", 307: "Temporary Redirect", 308: "Permanent Redirect",
    400: "Bad Request", 401: "Unauthorized", 402: "Payment Required", 403: "Forbidden",
    404: "Not Found", 405: "Method Not Allowed", 406: "Not Acceptable", 408: "Request Timeout",
    409: "Conflict", 410: "Gone", 411: "Length Required", 412: "Precondition Failed",
    413: "Payload Too Large", 414: "URI Too Long", 415: "Unsupported Media Type",
    422: "Unprocessable Entity", 428: "Precondition Required", 429: "Too Many Requests",
    500: "Internal Server Error", 501: "Not Implemented", 502: "Bad Gateway",
    503: "Service Unavailable", 504: "Gateway Timeout",
  };
  function reasonOf(r) {
    return r.statusText || REASONS[r.status] || "";
  }

  /** `application/x-www-form-urlencoded` of `[key, value]` pairs. */
  function formEncode(pairs) {
    const enc = (s) => encodeURIComponent(String(s)).replace(/%20/g, "+");
    return pairs.map(([k, v]) => enc(k) + "=" + enc(v)).join("&");
  }

  Object.assign(S, { vars, unsupported, utf8Encode, utf8Decode, atob, btoa, parseJson, responseAdapter, reasonOf, formEncode, textOf });
})();
