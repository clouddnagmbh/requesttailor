// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `expect`: the chai subset of api.d.ts (`Expect`), with chai's messages ("expected 404 to
// equal 200"), plus the response assertions Postman and Bruno scripts use
// (`.to.have.status(200)`, `.to.be.ok` on a response, `.to.have.header(...)`,
// `.to.have.jsonBody(...)`, `.to.have.jsonSchema(...)`).
//
// Loaded first of the shims (file-name order). Standard ECMAScript only, no host API.
// Internals shared with the other shims and modules live on the non-enumerable global
// `__rtShim` (`AssertionError`, `inspect`, `deepEqual`, `schemaErrors`, `RESPONSE`, `define`).

(function () {
  "use strict";
  const G = globalThis;

  /** Defines a global that user scripts may redeclare (`const res = …` must not clash). */
  function define(name, value) {
    Object.defineProperty(G, name, { value, writable: true, configurable: true, enumerable: false });
  }

  /** Objects that stand for an HTTP response carry this key (`pm.response`, Bruno's `res`,
   *  the result of `sendRequest`): `obj[RESPONSE]()` gives `{ code, reason, header(name),
   *  headerNames(), text(), json(), time }`. */
  const RESPONSE = Symbol.for("rt.shim.response");

  class AssertionError extends Error {
    constructor(message, props) {
      super(message);
      this.name = "AssertionError";
      if (props) Object.assign(this, props);
    }
  }

  // ---------------------------------------------------------------------------------------
  // Formatting as chai does (loupe-like `inspect`, `objDisplay` truncation at 40 chars)

  const IDENT_KEY = /^[A-Za-z_$][\w$]*$/;

  function quote(s) {
    return "'" + String(s).replace(/\\/g, "\\\\").replace(/'/g, "\\'").replace(/\n/g, "\\n") + "'";
  }

  function typeName(v) {
    return Object.prototype.toString.call(v).slice(8, -1);
  }

  function inspect(v, depth, seen) {
    depth = depth === undefined ? 2 : depth;
    seen = seen || [];
    if (v === null) return "null";
    if (v === undefined) return "undefined";
    switch (typeof v) {
      case "string":
        return quote(v);
      case "number":
        return Object.is(v, -0) ? "-0" : String(v);
      case "bigint":
        return v + "n";
      case "boolean":
        return String(v);
      case "symbol":
        return v.toString();
      case "function":
        return v.name ? "[Function " + v.name + "]" : "[Function]";
    }
    if (seen.indexOf(v) >= 0) return "[Circular]";
    const t = typeName(v);
    if (t === "Date") return isNaN(v.getTime()) ? "Invalid Date" : v.toISOString();
    if (t === "RegExp") return String(v);
    if (v instanceof Error) return "[" + (v.name || "Error") + ": " + v.message + "]";
    if (t === "Promise") return "Promise{…}";
    const next = seen.concat([v]);
    const item = (x) => (depth < 0 ? (Array.isArray(x) ? "[Array]" : typeof x === "object" && x ? "[Object]" : inspect(x, depth - 1, next)) : inspect(x, depth - 1, next));
    if (Array.isArray(v)) {
      if (v.length === 0) return "[]";
      return "[ " + v.map(item).join(", ") + " ]";
    }
    if (t === "Map") {
      if (v.size === 0) return "Map{}";
      return "Map{ " + Array.from(v, ([k, x]) => inspect(k, depth - 1, next) + " => " + item(x)).join(", ") + " }";
    }
    if (t === "Set") {
      if (v.size === 0) return "Set{}";
      return "Set{ " + Array.from(v, item).join(", ") + " }";
    }
    const keys = Object.keys(v);
    if (keys.length === 0) return "{}";
    return "{ " + keys.map((k) => (IDENT_KEY.test(k) ? k : quote(k)) + ": " + item(v[k])).join(", ") + " }";
  }

  function objDisplay(v) {
    const str = inspect(v);
    if (str.length < 40) return str;
    const t = typeName(v);
    if (t === "Function") return v.name ? "[Function: " + v.name + "]" : "[Function]";
    if (t === "Array") return "[ Array(" + v.length + ") ]";
    if (t === "Object") {
      const keys = Object.keys(v);
      const k = keys.length > 2 ? keys.slice(0, 2).join(", ") + ", ..." : keys.join(", ");
      return "{ Object (" + k + ") }";
    }
    return str;
  }

  // ---------------------------------------------------------------------------------------
  // Deep equality (chai's deep-eql semantics, without the exotic types)

  function deepEqual(a, b, seen) {
    if (a === b) return a !== 0 || 1 / a === 1 / b;
    if (a !== a && b !== b) return true; // NaN
    if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
    const ta = typeName(a);
    if (ta !== typeName(b)) return false;
    seen = seen || [];
    for (const [x, y] of seen) if (x === a && y === b) return true;
    seen.push([a, b]);
    switch (ta) {
      case "Date":
        return a.getTime() === b.getTime() || (isNaN(a.getTime()) && isNaN(b.getTime()));
      case "RegExp":
        return String(a) === String(b);
      case "Boolean":
      case "Number":
      case "String":
        return deepEqual(a.valueOf(), b.valueOf(), seen);
      case "Error":
        return a === b;
      case "Array":
        if (a.length !== b.length) return false;
        for (let i = 0; i < a.length; i++) if (!deepEqual(a[i], b[i], seen)) return false;
        return true;
      case "Set": {
        if (a.size !== b.size) return false;
        const rest = Array.from(b);
        for (const x of a) {
          const i = rest.findIndex((y) => deepEqual(x, y, seen));
          if (i < 0) return false;
          rest.splice(i, 1);
        }
        return true;
      }
      case "Map": {
        if (a.size !== b.size) return false;
        for (const [k, x] of a) if (!b.has(k) || !deepEqual(x, b.get(k), seen)) return false;
        return true;
      }
    }
    if (ArrayBuffer.isView(a)) {
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) return false;
      return true;
    }
    const ka = ownKeys(a);
    const kb = ownKeys(b);
    if (ka.length !== kb.length) return false;
    for (const k of ka) {
      if (!Object.prototype.hasOwnProperty.call(b, k)) return false;
      if (!deepEqual(a[k], b[k], seen)) return false;
    }
    return true;
  }

  function ownKeys(o) {
    const keys = [];
    for (const k in o) keys.push(k);
    return keys.concat(Object.getOwnPropertySymbols(o).filter((s) => Object.getOwnPropertyDescriptor(o, s).enumerable));
  }

  /** chai's type-detect, lower case: `null`, `array`, `object`, `string`, `date`, … */
  function typeOf(v) {
    if (v === null) return "null";
    if (v === undefined) return "undefined";
    if (Array.isArray(v)) return "array";
    const t = typeof v;
    if (t !== "object") return t;
    return typeName(v).toLowerCase();
  }

  // ---------------------------------------------------------------------------------------
  // JSON Schema (draft 4 to 2020-12, the keywords APIs use; `format` is not checked)

  function schemaErrors(schema, data) {
    const errors = [];
    const root = schema;
    const at = (path) => (path === "" ? "data" : "data" + path);
    const resolve = (ref) => {
      if (ref === "#") return root;
      if (!ref.startsWith("#/")) throw new Error("JSON Schema: only local $ref is supported, not " + ref);
      let node = root;
      for (const raw of ref.slice(2).split("/")) {
        const key = decodeURIComponent(raw).replace(/~1/g, "/").replace(/~0/g, "~");
        node = node == null ? undefined : node[key];
      }
      if (node === undefined) throw new Error("JSON Schema: cannot resolve $ref " + ref);
      return node;
    };
    const isType = (v, t) => {
      switch (t) {
        case "null":
          return v === null;
        case "array":
          return Array.isArray(v);
        case "object":
          return v !== null && typeof v === "object" && !Array.isArray(v);
        case "integer":
          return typeof v === "number" && Number.isInteger(v);
        case "number":
          return typeof v === "number" && isFinite(v);
        default:
          return typeof v === t;
      }
    };
    const check = (s, v, path, out) => {
      if (s === true || s === undefined) return;
      if (s === false) return void out.push(at(path) + " should not exist");
      if (s.$ref) return check(resolve(s.$ref), v, path, out);
      const before = out.length;
      if (s.nullable === true && v === null) return;
      if (s.type !== undefined) {
        const types = Array.isArray(s.type) ? s.type : [s.type];
        if (!types.some((t) => isType(v, t))) out.push(at(path) + " should be " + types.join(","));
      }
      if (s.enum && !s.enum.some((e) => deepEqual(e, v))) out.push(at(path) + " should be equal to one of the allowed values");
      if ("const" in s && !deepEqual(s.const, v)) out.push(at(path) + " should be equal to constant");
      if (typeof v === "string") {
        if (s.minLength !== undefined && [...v].length < s.minLength) out.push(at(path) + " should NOT be shorter than " + s.minLength + " characters");
        if (s.maxLength !== undefined && [...v].length > s.maxLength) out.push(at(path) + " should NOT be longer than " + s.maxLength + " characters");
        if (s.pattern !== undefined && !new RegExp(s.pattern, "u").test(v)) out.push(at(path) + ' should match pattern "' + s.pattern + '"');
      }
      if (typeof v === "number") {
        const exMin = s.exclusiveMinimum;
        const exMax = s.exclusiveMaximum;
        if (s.minimum !== undefined && (exMin === true ? v <= s.minimum : v < s.minimum)) out.push(at(path) + " should be >" + (exMin === true ? "" : "=") + " " + s.minimum);
        if (s.maximum !== undefined && (exMax === true ? v >= s.maximum : v > s.maximum)) out.push(at(path) + " should be <" + (exMax === true ? "" : "=") + " " + s.maximum);
        if (typeof exMin === "number" && v <= exMin) out.push(at(path) + " should be > " + exMin);
        if (typeof exMax === "number" && v >= exMax) out.push(at(path) + " should be < " + exMax);
        if (s.multipleOf !== undefined && Math.abs(v / s.multipleOf - Math.round(v / s.multipleOf)) > 1e-9) out.push(at(path) + " should be multiple of " + s.multipleOf);
      }
      if (Array.isArray(v)) {
        if (s.minItems !== undefined && v.length < s.minItems) out.push(at(path) + " should NOT have fewer than " + s.minItems + " items");
        if (s.maxItems !== undefined && v.length > s.maxItems) out.push(at(path) + " should NOT have more than " + s.maxItems + " items");
        if (s.uniqueItems) {
          for (let i = 0; i < v.length; i++)
            for (let j = i + 1; j < v.length; j++)
              if (deepEqual(v[i], v[j])) {
                out.push(at(path) + " should NOT have duplicate items (items ## " + j + " and " + i + " are identical)");
                i = j = v.length;
              }
        }
        const tuple = Array.isArray(s.items) ? s.items : s.prefixItems;
        if (Array.isArray(tuple)) {
          tuple.forEach((t, i) => i < v.length && check(t, v[i], path + "[" + i + "]", out));
          const extra = Array.isArray(s.items) ? s.additionalItems : s.items;
          if (extra !== undefined)
            for (let i = tuple.length; i < v.length; i++) check(extra, v[i], path + "[" + i + "]", out);
        } else if (s.items !== undefined) {
          v.forEach((x, i) => check(s.items, x, path + "[" + i + "]", out));
        }
        if (s.contains !== undefined && !v.some((x) => { const e = []; check(s.contains, x, path, e); return e.length === 0; }))
          out.push(at(path) + " should contain a valid item");
      }
      if (isType(v, "object")) {
        const keys = Object.keys(v);
        if (s.minProperties !== undefined && keys.length < s.minProperties) out.push(at(path) + " should NOT have fewer than " + s.minProperties + " properties");
        if (s.maxProperties !== undefined && keys.length > s.maxProperties) out.push(at(path) + " should NOT have more than " + s.maxProperties + " properties");
        for (const r of s.required || []) if (!(r in v)) out.push(at(path) + " should have required property '" + r + "'");
        const props = s.properties || {};
        const patterns = Object.keys(s.patternProperties || {});
        for (const k of keys) {
          const sub = path + (IDENT_KEY.test(k) ? "." + k : "['" + k + "']");
          let matched = false;
          if (Object.prototype.hasOwnProperty.call(props, k)) {
            matched = true;
            check(props[k], v[k], sub, out);
          }
          for (const p of patterns)
            if (new RegExp(p, "u").test(k)) {
              matched = true;
              check(s.patternProperties[p], v[k], sub, out);
            }
          if (!matched && s.additionalProperties !== undefined) {
            if (s.additionalProperties === false) out.push(at(path) + " should NOT have additional properties ('" + k + "')");
            else check(s.additionalProperties, v[k], sub, out);
          }
          if (s.propertyNames !== undefined) check(s.propertyNames, k, sub, out);
        }
        for (const [k, dep] of Object.entries(s.dependencies || s.dependentRequired || {}))
          if (k in v) {
            if (Array.isArray(dep)) for (const d of dep) !(d in v) && out.push(at(path) + " should have property " + d + " when property " + k + " is present");
            else check(dep, v, path, out);
          }
      }
      for (const sub of s.allOf || []) check(sub, v, path, out);
      const passes = (sub) => {
        const e = [];
        check(sub, v, path, e);
        return e.length === 0;
      };
      if (s.anyOf && !s.anyOf.some(passes)) out.push(at(path) + " should match some schema in anyOf");
      if (s.oneOf) {
        const n = s.oneOf.filter(passes).length;
        if (n !== 1) out.push(at(path) + " should match exactly one schema in oneOf");
      }
      if (s.not !== undefined && passes(s.not)) out.push(at(path) + " should NOT be valid");
      if (s.if !== undefined) {
        if (passes(s.if)) s.then !== undefined && check(s.then, v, path, out);
        else s.else !== undefined && check(s.else, v, path, out);
      }
      return out.length === before;
    };
    check(schema, data, "", errors);
    return errors;
  }

  // ---------------------------------------------------------------------------------------
  // Responses

  /** The response adapter of `v`, or `undefined` if `v` is not a response. `rt.response`
   *  itself and objects shaped like it count too. */
  function responseOf(v) {
    if (v == null || (typeof v !== "object" && typeof v !== "function")) return undefined;
    if (typeof v[RESPONSE] === "function") return v[RESPONSE]();
    if (typeof v.status === "number" && v.headers && typeof v.headers.get === "function" && typeof v.text === "function") {
      return {
        code: v.status,
        reason: v.statusText,
        header: (n) => v.headers.get(n),
        text: () => v.text(),
        json: () => v.json(),
        time: v.time,
      };
    }
    return undefined;
  }

  // ---------------------------------------------------------------------------------------
  // Assertion

  const CHAINS = ["to", "be", "been", "is", "that", "which", "and", "has", "have", "with", "at", "of", "same", "but", "does", "still", "also"];

  function Assertion(obj, message, flags) {
    this.__flags = Object.assign({ object: obj, message }, flags || {});
  }

  function flag(a, name, value) {
    if (arguments.length === 3) a.__flags[name] = value;
    return a.__flags[name];
  }

  /** chai's `assert`: `pass` true means "as expected without `not`". */
  function check(a, pass, msg, negMsg, expected, actual) {
    const negate = flag(a, "negate");
    if (negate ? pass : !pass) {
      const obj = flag(a, "object");
      const fill = (m) =>
        m
          .replace(/#\{this\}/g, () => objDisplay(obj))
          .replace(/#\{exp\}/g, () => objDisplay(expected))
          .replace(/#\{act\}/g, () => objDisplay(actual === undefined ? obj : actual));
      let text = fill(negate ? negMsg : msg);
      const custom = flag(a, "message");
      if (custom) text = custom + ": " + text;
      throw new AssertionError(text, { actual: actual === undefined ? obj : actual, expected, showDiff: !negate });
    }
  }

  function withMessage(a, msg) {
    if (msg !== undefined) flag(a, "message", msg);
  }

  const proto = Assertion.prototype;

  for (const word of CHAINS) {
    Object.defineProperty(proto, word, { get() { return this; }, configurable: true });
  }

  function addFlag(name, key, value) {
    Object.defineProperty(proto, name, {
      get() {
        flag(this, key, value === undefined ? true : value);
        return this;
      },
      configurable: true,
    });
  }
  addFlag("not", "negate");
  addFlag("deep", "deep");
  addFlag("nested", "nested");
  addFlag("own", "own");
  addFlag("ordered", "ordered");
  addFlag("any", "any");
  addFlag("all", "all");

  function addProperty(name, fn) {
    Object.defineProperty(proto, name, {
      get() {
        fn.call(this);
        return this;
      },
      configurable: true,
    });
  }

  function addMethod(names, fn) {
    for (const name of [].concat(names)) {
      Object.defineProperty(proto, name, {
        value: function () {
          const r = fn.apply(this, arguments);
          return r === undefined ? this : r;
        },
        writable: true,
        configurable: true,
      });
    }
  }

  /** A word that is a method and a chain at once (`include`, `length`): as a property it
   *  sets `chainFlag` and returns a callable that runs `fn`. */
  function addChainable(names, fn, chainFlag) {
    for (const name of [].concat(names)) {
      Object.defineProperty(proto, name, {
        get() {
          const self = this;
          flag(self, chainFlag, true);
          const callable = function () {
            const r = fn.apply(self, arguments);
            return r === undefined ? self : r;
          };
          Object.setPrototypeOf(callable, self);
          return callable;
        },
        configurable: true,
      });
    }
  }

  function subject(a) {
    return flag(a, "object");
  }

  function isResponseAssertion(a) {
    return responseOf(subject(a));
  }

  // --- properties
  addProperty("ok", function () {
    const r = isResponseAssertion(this);
    if (r) {
      check(this, r.code >= 200 && r.code < 300, "expected response code to be 2XX but found " + r.code, "expected response code to not be 2XX but found " + r.code);
      return;
    }
    check(this, !!subject(this), "expected #{this} to be truthy", "expected #{this} to be falsy");
  });
  addProperty("true", function () {
    check(this, subject(this) === true, "expected #{this} to be true", "expected #{this} to be false");
  });
  addProperty("false", function () {
    check(this, subject(this) === false, "expected #{this} to be false", "expected #{this} to be true");
  });
  addProperty("null", function () {
    check(this, subject(this) === null, "expected #{this} to be null", "expected #{this} not to be null");
  });
  addProperty("undefined", function () {
    check(this, subject(this) === undefined, "expected #{this} to be undefined", "expected #{this} not to be undefined");
  });
  addProperty("NaN", function () {
    check(this, Number.isNaN(subject(this)), "expected #{this} to be NaN", "expected #{this} not to be NaN");
  });
  addProperty("finite", function () {
    check(this, typeof subject(this) === "number" && isFinite(subject(this)), "expected #{this} to be a finite number", "expected #{this} to not be a finite number");
  });
  addProperty("exist", function () {
    const v = subject(this);
    check(this, v !== null && v !== undefined, "expected #{this} to exist", "expected #{this} to not exist");
  });
  addProperty("exists", function () {
    const v = subject(this);
    check(this, v !== null && v !== undefined, "expected #{this} to exist", "expected #{this} to not exist");
  });
  addProperty("empty", function () {
    const v = subject(this);
    let n;
    if (typeof v === "string" || Array.isArray(v) || ArrayBuffer.isView(v)) n = v.length;
    else if (v instanceof Map || v instanceof Set) n = v.size;
    else if (v !== null && typeof v === "object") n = Object.keys(v).length;
    else throw new TypeError(".empty was passed non-string primitive " + inspect(v));
    check(this, n === 0, "expected #{this} to be empty", "expected #{this} not to be empty");
  });

  // Postman response classes (chai-postman).
  const RESPONSE_CLASSES = {
    success: [200, 299, "2XX"],
    info: [100, 199, "1XX"],
    redirection: [300, 399, "3XX"],
    clientError: [400, 499, "4XX"],
    serverError: [500, 599, "5XX"],
    error: [400, 599, "4XX or 5XX"],
  };
  for (const [name, [lo, hi, label]] of Object.entries(RESPONSE_CLASSES)) {
    addProperty(name, function () {
      const r = needResponse(this, name);
      check(this, r.code >= lo && r.code <= hi, "expected response code to be " + label + " but found " + r.code, "expected response code to not be " + label + " but found " + r.code);
    });
  }
  const RESPONSE_CODES = { accepted: 202, withoutContent: 204, badRequest: 400, unauthorized: 401, unauthorised: 401, forbidden: 403, notFound: 404, notAcceptable: 406, rateLimited: 429 };
  for (const [name, code] of Object.entries(RESPONSE_CODES)) {
    addProperty(name, function () {
      const r = needResponse(this, name);
      check(this, r.code === code, "expected response code to be " + code + " but found " + r.code, "expected response code to not be " + code + " but found " + r.code);
    });
  }
  addProperty("json", function () {
    const r = needResponse(this, "json");
    let ok = true;
    try {
      r.json();
    } catch (e) {
      ok = false;
    }
    check(this, ok, "expected response body to be a valid json", "expected response body not to be a valid json");
  });
  addProperty("withBody", function () {
    const r = needResponse(this, "withBody");
    check(this, r.text().length > 0, "expected response to have content in body", "expected response to not have content in body");
  });
  for (const [name, re] of [["html", /html/i], ["xml", /xml/i]]) {
    addProperty(name, function () {
      const r = needResponse(this, name);
      const ct = r.header("content-type") || "";
      check(this, re.test(ct), "expected response to be " + name + " but got content type '" + ct + "'", "expected response to not be " + name);
    });
  }

  function needResponse(a, what) {
    const r = isResponseAssertion(a);
    if (!r) throw new TypeError("." + what + " needs a response (pm.response, res or a sendRequest result), not " + inspect(subject(a)));
    return r;
  }

  // --- methods
  function assertEqual(val, msg) {
    withMessage(this, msg);
    const obj = subject(this);
    if (flag(this, "deep")) return assertEql.call(this, val);
    check(this, val === obj, "expected #{this} to equal #{exp}", "expected #{this} to not equal #{exp}", val, obj);
  }
  addMethod(["equal", "equals", "eq"], assertEqual);

  function assertEql(val, msg) {
    withMessage(this, msg);
    check(this, deepEqual(subject(this), val), "expected #{this} to deeply equal #{exp}", "expected #{this} to not deeply equal #{exp}", val, subject(this));
  }
  addMethod(["eql", "eqls"], assertEql);

  function numeric(a, n, what) {
    const obj = subject(a);
    if (flag(a, "doLength")) {
      const len = obj == null ? undefined : obj.length !== undefined ? obj.length : obj.size;
      if (typeof len !== "number") throw new TypeError("expected " + inspect(obj) + " to have a property 'length'");
      return len;
    }
    if (typeof obj !== "number" && !(obj instanceof Date)) {
      const custom = flag(a, "message");
      throw new AssertionError((custom ? custom + ": " : "") + "expected " + inspect(obj) + " to be a number or a date");
    }
    if (typeof n !== "number" && !(n instanceof Date)) throw new TypeError("the argument to " + what + " must be a number");
    return obj;
  }

  function compare(names, test, word, negWord) {
    addMethod(names, function (n, msg) {
      withMessage(this, msg);
      const v = numeric(this, n, names[0]);
      if (flag(this, "doLength"))
        check(this, test(v, n), "expected #{this} to have a length " + word + " #{exp} but got #{act}", "expected #{this} to not have a length " + word + " #{exp}", n, v);
      else check(this, test(v, n), "expected #{this} to be " + word + " #{exp}", "expected #{this} to be " + negWord + " #{exp}", n);
    });
  }
  compare(["above", "gt", "greaterThan"], (v, n) => v > n, "above", "at most");
  compare(["least", "gte", "greaterThanOrEqual"], (v, n) => v >= n, "at least", "below");
  compare(["below", "lt", "lessThan"], (v, n) => v < n, "below", "at least");
  compare(["most", "lte", "lessThanOrEqual"], (v, n) => v <= n, "at most", "above");

  addMethod("within", function (lo, hi, msg) {
    withMessage(this, msg);
    const v = numeric(this, lo, "within");
    const range = lo + ".." + hi;
    if (flag(this, "doLength"))
      check(this, v >= lo && v <= hi, "expected #{this} to have a length within " + range, "expected #{this} to not have a length within " + range);
    else check(this, v >= lo && v <= hi, "expected #{this} to be within " + range, "expected #{this} to not be within " + range);
  });

  addMethod(["closeTo", "approximately"], function (expected, delta, msg) {
    withMessage(this, msg);
    const v = subject(this);
    check(this, Math.abs(v - expected) <= delta, "expected #{this} to be close to " + expected + " +/- " + delta, "expected #{this} not to be close to " + expected + " +/- " + delta);
  });

  function assertType(type, msg) {
    withMessage(this, msg);
    type = String(type).toLowerCase();
    const article = /^[aeiou]/.test(type) ? "an " : "a ";
    check(this, typeOf(subject(this)) === type, "expected #{this} to be " + article + type, "expected #{this} not to be " + article + type);
  }
  addChainable(["a", "an"], assertType, "typeChain");

  function assertInclude(val, msg) {
    withMessage(this, msg);
    const obj = subject(this);
    const deep = flag(this, "deep");
    const descriptor = deep ? "deep " : "";
    let pass;
    if (typeof obj === "string") {
      if (typeof val !== "string") throw new AssertionError("the given combination of arguments (string and " + typeOf(val) + ") is invalid for this assertion. You can use an array, a map, an object, a set, a string, or a weakset instead of a " + typeOf(val));
      pass = obj.indexOf(val) !== -1;
    } else if (Array.isArray(obj)) {
      pass = deep ? obj.some((x) => deepEqual(x, val)) : obj.indexOf(val) !== -1 || (val !== val && obj.some((x) => x !== x));
    } else if (obj instanceof Set) {
      pass = deep ? Array.from(obj).some((x) => deepEqual(x, val)) : obj.has(val);
    } else if (obj instanceof Map) {
      pass = Array.from(obj.values()).some((x) => (deep ? deepEqual(x, val) : x === val));
    } else if (obj !== null && typeof obj === "object" && val !== null && typeof val === "object") {
      pass = Object.keys(val).every((k) => k in obj && (deep ? deepEqual(obj[k], val[k]) : obj[k] === val[k]));
    } else {
      throw new AssertionError("object tested must be an array, a map, an object, a set, a string, or a weakset, but " + typeOf(obj) + " given");
    }
    check(this, pass, "expected #{this} to " + descriptor + "include #{exp}", "expected #{this} to not " + descriptor + "include #{exp}", val);
  }
  addChainable(["include", "includes", "contain", "contains"], assertInclude, "contains");

  addMethod("oneOf", function (list, msg) {
    withMessage(this, msg);
    const obj = subject(this);
    const pass = flag(this, "contains")
      ? list.some((x) => (typeof obj === "string" ? obj.indexOf(x) !== -1 : Array.isArray(obj) && obj.indexOf(x) !== -1))
      : list.some((x) => (flag(this, "deep") ? deepEqual(x, obj) : x === obj));
    check(this, pass, "expected #{this} to be one of #{exp}", "expected #{this} to not be one of #{exp}", list);
  });

  function nestedGet(obj, path) {
    const parts = [];
    String(path).replace(/\[(\d+)\]|\[['"]([^'"]*)['"]\]|([^.[\]]+)/g, (m, i, q, k) => {
      parts.push(i !== undefined ? Number(i) : q !== undefined ? q : k);
      return m;
    });
    let cur = obj;
    for (let i = 0; i < parts.length; i++) {
      if (cur == null || !(parts[i] in Object(cur))) return { exists: false };
      cur = cur[parts[i]];
    }
    return { exists: true, value: cur };
  }

  function assertProperty(name, val, msg) {
    withMessage(this, msg);
    const hasValue = arguments.length > 1;
    const obj = subject(this);
    const nested = flag(this, "nested");
    const own = flag(this, "own");
    const deep = flag(this, "deep");
    if (obj === null || obj === undefined) {
      const custom = flag(this, "message");
      throw new AssertionError((custom ? custom + ": " : "") + "Target cannot be null or undefined.");
    }
    let found;
    if (nested) found = nestedGet(obj, name);
    else if (own) found = { exists: Object.prototype.hasOwnProperty.call(obj, name), value: obj[name] };
    else found = { exists: name in Object(obj), value: obj[name] };
    const descriptor = (deep ? "deep " : "") + (own ? "own " : "") + (nested ? "nested " : "");
    if (!flag(this, "negate") || !hasValue)
      check(this, found.exists, "expected #{this} to have " + descriptor + "property " + inspect(name), "expected #{this} to not have " + descriptor + "property " + inspect(name));
    if (hasValue)
      check(this, found.exists && (deep ? deepEqual(val, found.value) : val === found.value), "expected #{this} to have " + descriptor + "property " + inspect(name) + " of #{exp}, but got #{act}", "expected #{this} to not have " + descriptor + "property " + inspect(name) + " of #{act}", val, found.value);
    flag(this, "object", found.value);
  }
  addMethod("property", assertProperty);
  addMethod(["ownProperty", "haveOwnProperty"], function (name, val, msg) {
    flag(this, "own", true);
    return assertProperty.apply(this, arguments);
  });
  addMethod("nestedProperty", function (name, val, msg) {
    flag(this, "nested", true);
    return assertProperty.apply(this, arguments);
  });

  function assertLength(n, msg) {
    withMessage(this, msg);
    const obj = subject(this);
    const len = obj == null ? undefined : obj instanceof Map || obj instanceof Set ? obj.size : obj.length;
    check(this, len === n, "expected #{this} to have a length of #{exp} but got #{act}", "expected #{this} to not have a length of #{act}", n, len);
  }
  addChainable(["length", "lengthOf"], assertLength, "doLength");

  addMethod(["match", "matches"], function (re, msg) {
    withMessage(this, msg);
    check(this, re.exec(subject(this)) !== null, "expected #{this} to match " + re, "expected #{this} not to match " + re);
  });

  addMethod("string", function (str, msg) {
    withMessage(this, msg);
    check(this, String(subject(this)).indexOf(str) !== -1, "expected #{this} to contain " + inspect(str), "expected #{this} to not contain " + inspect(str));
  });

  function keyList(obj) {
    if (obj instanceof Map || obj instanceof Set) return Array.from(obj.keys());
    if (obj === null || typeof obj !== "object") return [];
    return Object.keys(obj);
  }

  addMethod(["keys", "key"], function (...args) {
    const obj = subject(this);
    let keys = args;
    if (args.length === 1 && Array.isArray(args[0])) keys = args[0];
    else if (args.length === 1 && args[0] !== null && typeof args[0] === "object") keys = Object.keys(args[0]);
    if (!keys.length) throw new Error("keys required");
    const actual = keyList(obj);
    const deep = flag(this, "deep");
    const has = (k) => actual.some((a) => (deep ? deepEqual(a, k) : a === k));
    const any = flag(this, "any");
    const all = flag(this, "all") || !any;
    let pass;
    if (any) pass = keys.some(has);
    if (all) {
      pass = keys.every(has);
      if (!flag(this, "contains")) pass = pass && keys.length === actual.length;
    }
    const shown = keys.map(inspect);
    let str;
    if (shown.length > 1) {
      const last = shown.pop();
      str = all ? shown.join(", ") + ", and " + last : shown.join(", ") + ", or " + last;
    } else str = shown[0];
    str = (keys.length > 1 ? "keys " : "key ") + str;
    str = (flag(this, "contains") ? "contain " : "have ") + str;
    const d = deep ? "deep " : "";
    check(this, pass, "expected #{this} to " + d + str, "expected #{this} to not " + d + str, keys.slice().sort(), actual.slice().sort());
  });

  addMethod("members", function (list, msg) {
    withMessage(this, msg);
    const obj = subject(this);
    if (!Array.isArray(obj) || !Array.isArray(list)) throw new AssertionError("expected " + inspect(obj) + " and " + inspect(list) + " to be arrays");
    const deep = flag(this, "deep");
    const eq = (a, b) => (deep ? deepEqual(a, b) : a === b);
    const ordered = flag(this, "ordered");
    const contains = flag(this, "contains");
    let pass;
    if (ordered) {
      pass = contains ? list.every((x, i) => eq(obj[i], x)) : list.length === obj.length && list.every((x, i) => eq(obj[i], x));
    } else {
      const rest = obj.slice();
      pass = list.every((x) => {
        const i = rest.findIndex((y) => eq(y, x));
        if (i < 0) return false;
        rest.splice(i, 1);
        return true;
      });
      if (!contains) pass = pass && obj.length === list.length;
    }
    const what = (deep ? "deep " : "") + (ordered ? "ordered members" : "members");
    if (contains)
      check(this, pass, "expected #{this} to be " + (ordered ? "an ordered " : "a ") + "superset of #{exp}", "expected #{this} to not be " + (ordered ? "an ordered " : "a ") + "superset of #{exp}", list);
    else check(this, pass, "expected #{this} to have the same " + what + " as #{exp}", "expected #{this} to not have the same " + what + " as #{exp}", list);
  });

  addMethod(["instanceOf", "instanceof"], function (ctor, msg) {
    withMessage(this, msg);
    const name = (ctor && ctor.name) || inspect(ctor);
    check(this, subject(this) instanceof ctor, "expected #{this} to be an instance of " + name, "expected #{this} to not be an instance of " + name);
  });

  addMethod(["throw", "throws", "Throw"], function (errorLike, errMsg, msg) {
    if (typeof errorLike === "string" || errorLike instanceof RegExp) {
      msg = errMsg;
      errMsg = errorLike;
      errorLike = undefined;
    }
    withMessage(this, msg);
    const fn = subject(this);
    if (typeof fn !== "function") throw new AssertionError("expected " + inspect(fn) + " to be a function");
    let thrown;
    let threw = false;
    try {
      fn();
    } catch (e) {
      threw = true;
      thrown = e;
    }
    const negate = flag(this, "negate");
    if (errorLike === undefined && errMsg === undefined) {
      check(this, threw, "expected #{this} to throw an error", "expected #{this} to not throw an error but #{act} was thrown", undefined, thrown);
      return;
    }
    if (errorLike !== undefined) {
      const ok = threw && (typeof errorLike === "function" ? thrown instanceof errorLike : thrown === errorLike);
      const name = typeof errorLike === "function" ? errorLike.name : inspect(errorLike);
      if (!negate || errMsg === undefined)
        check(this, ok, "expected #{this} to throw " + name + (threw ? " but #{act} was thrown" : ""), "expected #{this} to not throw " + name + " but #{act} was thrown", errorLike, thrown);
    }
    if (errMsg !== undefined) {
      const text = thrown && thrown.message !== undefined ? String(thrown.message) : String(thrown);
      const ok = threw && (errMsg instanceof RegExp ? errMsg.test(text) : text.indexOf(errMsg) !== -1);
      const how = errMsg instanceof RegExp ? "matching " : "including ";
      check(this, ok, "expected #{this} to throw error " + how + inspect(errMsg) + " but got " + inspect(text), "expected #{this} to throw error not " + how + inspect(errMsg), errMsg, text);
    }
    flag(this, "object", thrown);
  });

  addMethod(["satisfy", "satisfies"], function (fn, msg) {
    withMessage(this, msg);
    const r = fn(subject(this));
    check(this, !!r, "expected #{this} to satisfy " + objDisplay(fn), "expected #{this} to not satisfy" + objDisplay(fn), !flag(this, "negate"), r);
  });

  // --- responses (Postman `pm.response.to.have.status(200)`, Bruno `expect(res)…`)
  addMethod(["status", "statusCode"], function (code) {
    const r = needResponse(this, "status");
    if (typeof code === "string") {
      check(this, String(r.reason) === code, "expected response to have status reason " + quote(code) + " but got " + quote(r.reason), "expected response to not have status reason " + quote(code));
    } else {
      check(this, r.code === code, "expected response to have status code " + code + " but got " + r.code, "expected response to not have status code " + code);
    }
  });

  addMethod("header", function (name, value) {
    const r = needResponse(this, "header");
    const actual = r.header(name);
    if (arguments.length < 2 || flag(this, "negate")) {
      if (arguments.length < 2 || actual === undefined)
        check(this, actual !== undefined, "expected response to have header with key " + quote(name), "expected response to not have header with key " + quote(name));
      if (arguments.length < 2) return;
    }
    check(this, actual !== undefined && String(actual).toLowerCase() === String(value).toLowerCase(), "expected " + quote(name) + " response header to be " + quote(value) + " but got " + quote(actual), "expected " + quote(name) + " response header to not be " + quote(value));
  });

  addMethod("body", function (content) {
    const r = needResponse(this, "body");
    const text = r.text();
    if (arguments.length === 0) {
      check(this, text.length > 0, "expected response to have content in body", "expected response to not have content in body");
    } else if (content instanceof RegExp) {
      check(this, content.test(text), "expected response body text to match " + content + " but got " + quote(text), "expected response body text to not match " + content);
    } else if (typeof content === "string") {
      check(this, text === content, "expected response body to equal " + quote(content) + " but got " + quote(text), "expected response body to not equal " + quote(content));
    } else {
      let json;
      try {
        json = r.json();
      } catch (e) {
        json = undefined;
      }
      check(this, deepEqual(json, content), "expected response body json to equal " + objDisplay(content) + " but got " + objDisplay(json), "expected response body json to not equal " + objDisplay(content));
    }
  });

  addMethod("jsonBody", function (path, value) {
    const r = needResponse(this, "jsonBody");
    let json;
    let error;
    try {
      json = r.json();
    } catch (e) {
      error = e;
    }
    if (error || arguments.length === 0) {
      check(this, !error, "expected response body to be a valid json but got error " + (error && error.message), "expected response body not to be a valid json");
      return;
    }
    if (typeof path !== "string") {
      check(this, deepEqual(json, path), "expected response body json to equal " + objDisplay(path) + " but got " + objDisplay(json), "expected response body json to not equal " + objDisplay(path));
      return;
    }
    const found = nestedGet(json, path);
    if (arguments.length === 1) {
      check(this, found.exists, "expected " + objDisplay(json) + " to have nested property " + quote(path), "expected " + objDisplay(json) + " to not have nested property " + quote(path));
      return;
    }
    check(this, found.exists && deepEqual(found.value, value), "expected response body json at " + quote(path) + " to contain " + objDisplay(value) + " but got " + objDisplay(found.value), "expected response body json at " + quote(path) + " to not contain " + objDisplay(value));
  });

  addMethod("jsonSchema", function (schema) {
    const r = responseOf(subject(this));
    let data = subject(this);
    if (r) {
      try {
        data = r.json();
      } catch (e) {
        check(this, false, "expected response body to be a valid json but got error " + e.message, "expected response body not to be a valid json");
      }
    }
    const errors = schemaErrors(schema, data);
    check(this, errors.length === 0, "expected data to satisfy schema but found following errors: \n" + errors.join(", "), "expected data to not satisfy schema");
  });

  addMethod("responseTime", function () {
    const r = needResponse(this, "responseTime");
    flag(this, "object", r.time);
  });

  // ---------------------------------------------------------------------------------------

  function expect(value, message) {
    return new Assertion(value, message);
  }
  expect.fail = function (actual, expected, message) {
    if (arguments.length < 2) {
      message = actual;
      actual = undefined;
    }
    throw new AssertionError(message === undefined ? "expect.fail()" : String(message), { actual, expected });
  };
  expect.AssertionError = AssertionError;
  expect.Assertion = Assertion;

  define("expect", expect);
  define("__rtShim", {
    AssertionError,
    Assertion,
    RESPONSE,
    define,
    deepEqual,
    inspect,
    objDisplay,
    responseOf,
    schemaErrors,
    typeOf,
    nestedGet,
  });
})();
