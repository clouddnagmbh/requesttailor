// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('lodash')` (and Postman's global `_`): the lodash 4 functions request scripts use,
// written for RequestTailor (not the lodash code). Iteratee shorthands work where lodash has
// them: a function, a property name (`'id'`), a matches object (`{ id: 1 }`) or a
// `[path, value]` pair. Functions not listed in SHIM-COVERAGE.md are missing.

"use strict";

const hasOwn = (o, k) => o != null && Object.prototype.hasOwnProperty.call(o, k);
const tag = (v) => Object.prototype.toString.call(v);

function toPath(path) {
  if (Array.isArray(path)) return path;
  const out = [];
  String(path).replace(/\[(\d+)\]|\[['"]([^'"]*)['"]\]|([^.[\]]+)/g, (m, i, q, k) => {
    out.push(i !== undefined ? i : q !== undefined ? q : k);
    return m;
  });
  return out;
}

function get(obj, path, dflt) {
  let cur = obj;
  for (const k of toPath(path)) {
    if (cur == null) return dflt;
    cur = cur[k];
  }
  return cur === undefined ? dflt : cur;
}

function has(obj, path) {
  let cur = obj;
  for (const k of toPath(path)) {
    if (!hasOwn(cur, k)) return false;
    cur = cur[k];
  }
  return true;
}

function set(obj, path, value) {
  const p = toPath(path);
  let cur = obj;
  p.forEach((k, i) => {
    if (i === p.length - 1) cur[k] = value;
    else {
      if (cur[k] == null || typeof cur[k] !== "object") cur[k] = /^\d+$/.test(p[i + 1]) ? [] : {};
      cur = cur[k];
    }
  });
  return obj;
}

function isEqual(a, b) {
  if (a === b) return true;
  if (a !== a && b !== b) return true;
  if (typeof a !== "object" || typeof b !== "object" || a === null || b === null) return false;
  if (tag(a) !== tag(b)) return false;
  if (a instanceof Date) return a.getTime() === b.getTime();
  if (a instanceof RegExp) return String(a) === String(b);
  if (Array.isArray(a)) return a.length === b.length && a.every((x, i) => isEqual(x, b[i]));
  if (a instanceof Map || a instanceof Set) return isEqual(Array.from(a), Array.from(b));
  const ka = Object.keys(a);
  const kb = Object.keys(b);
  return ka.length === kb.length && ka.every((k) => hasOwn(b, k) && isEqual(a[k], b[k]));
}

function isMatch(obj, src) {
  return Object.keys(src).every((k) => {
    const v = src[k];
    if (v !== null && typeof v === "object" && obj != null && typeof obj[k] === "object") return isMatch(obj[k], v);
    return obj != null && isEqual(obj[k], v);
  });
}

function iteratee(f) {
  if (typeof f === "function") return f;
  if (f == null) return (x) => x;
  if (Array.isArray(f)) return (x) => isEqual(get(x, f[0]), f[1]);
  if (typeof f === "object") return (x) => isMatch(x, f);
  return (x) => get(x, f);
}

const values = (o) => (o == null ? [] : Array.isArray(o) || typeof o === "string" ? Array.from(o) : Object.keys(o).map((k) => o[k]));
const entriesOf = (o) => (Array.isArray(o) ? o.map((v, i) => [i, v]) : Object.keys(o || {}).map((k) => [k, o[k]]));

function forEach(c, f) {
  for (const [k, v] of entriesOf(c)) if (f(v, k, c) === false) break;
  return c;
}

function cloneDeep(v) {
  if (v === null || typeof v !== "object") return v;
  if (v instanceof Date) return new Date(v.getTime());
  if (v instanceof RegExp) return new RegExp(v.source, v.flags);
  if (Array.isArray(v)) return v.map(cloneDeep);
  if (v instanceof Map) return new Map(Array.from(v, ([k, x]) => [k, cloneDeep(x)]));
  if (v instanceof Set) return new Set(Array.from(v, cloneDeep));
  const out = {};
  for (const k of Object.keys(v)) out[k] = cloneDeep(v[k]);
  return out;
}

function isPlainObject(v) {
  if (tag(v) !== "[object Object]") return false;
  const p = Object.getPrototypeOf(v);
  return p === null || p === Object.prototype || Object.getPrototypeOf(p) === null;
}

function merge(target, ...sources) {
  for (const s of sources) {
    if (s == null) continue;
    for (const k of Object.keys(s)) {
      const v = s[k];
      if (v !== null && typeof v === "object" && (isPlainObject(v) || Array.isArray(v))) {
        if (target[k] == null || typeof target[k] !== "object") target[k] = Array.isArray(v) ? [] : {};
        merge(target[k], v);
      } else if (v !== undefined) target[k] = v;
    }
  }
  return target;
}

function compareBy(fns, orders) {
  return (a, b) => {
    for (let i = 0; i < fns.length; i++) {
      const x = fns[i](a);
      const y = fns[i](b);
      const dir = orders && String(orders[i]).toLowerCase() === "desc" ? -1 : 1;
      if (x < y || (x === undefined && y !== undefined)) return -dir;
      if (x > y || (y === undefined && x !== undefined)) return dir;
    }
    return 0;
  };
}

const words = (s) => String(s).replace(/([a-z\d])([A-Z])/g, "$1 $2").replace(/([A-Z]+)([A-Z][a-z])/g, "$1 $2").match(/[A-Za-z\d]+/g) || [];
const cap = (w) => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase();

function flattenDepth(a, d) {
  return d < 1 ? a.slice() : a.reduce((acc, x) => acc.concat(Array.isArray(x) ? flattenDepth(x, d - 1) : [x]), []);
}

const _ = {
  // Objects and paths
  get,
  set,
  has,
  keys: (o) => (o == null ? [] : Object.keys(o)),
  values,
  entries: (o) => Object.entries(o || {}),
  toPairs: (o) => Object.entries(o || {}),
  fromPairs: (p) => Object.fromEntries(p || []),
  pick: (o, ...ks) => Object.fromEntries(ks.flat().filter((k) => has(o, k)).map((k) => [k, get(o, k)])),
  omit(o, ...ks) {
    const drop = new Set(ks.flat().map(String));
    return Object.fromEntries(Object.keys(o || {}).filter((k) => !drop.has(k)).map((k) => [k, o[k]]));
  },
  pickBy: (o, f) => Object.fromEntries(Object.keys(o || {}).filter((k) => iteratee(f)(o[k], k)).map((k) => [k, o[k]])),
  omitBy: (o, f) => Object.fromEntries(Object.keys(o || {}).filter((k) => !iteratee(f)(o[k], k)).map((k) => [k, o[k]])),
  mapValues: (o, f) => Object.fromEntries(Object.keys(o || {}).map((k) => [k, iteratee(f)(o[k], k, o)])),
  mapKeys: (o, f) => Object.fromEntries(Object.keys(o || {}).map((k) => [iteratee(f)(o[k], k, o), o[k]])),
  assign: (t, ...s) => Object.assign(t, ...s),
  defaults(t, ...s) {
    for (const src of s) for (const k of Object.keys(src || {})) if (t[k] === undefined) t[k] = src[k];
    return t;
  },
  merge,
  clone: (v) => (Array.isArray(v) ? v.slice() : v !== null && typeof v === "object" ? Object.assign(Object.create(Object.getPrototypeOf(v)), v) : v),
  cloneDeep,
  invert: (o) => Object.fromEntries(Object.keys(o || {}).map((k) => [o[k], k])),
  transform(o, f, acc) {
    if (acc === undefined) acc = Array.isArray(o) ? [] : {};
    for (const [k, v] of entriesOf(o)) if (f(acc, v, k, o) === false) break;
    return acc;
  },
  // Types
  isEqual,
  isMatch,
  isEmpty(v) {
    if (v == null) return true;
    if (typeof v === "string" || Array.isArray(v)) return v.length === 0;
    if (v instanceof Map || v instanceof Set) return v.size === 0;
    if (typeof v === "object") return Object.keys(v).length === 0;
    return true;
  },
  isObject: (v) => v !== null && (typeof v === "object" || typeof v === "function"),
  isObjectLike: (v) => v !== null && typeof v === "object",
  isPlainObject,
  isArray: Array.isArray,
  isString: (v) => typeof v === "string",
  isNumber: (v) => typeof v === "number",
  isInteger: Number.isInteger,
  isFinite: Number.isFinite,
  isNaN: (v) => typeof v === "number" && v !== v,
  isBoolean: (v) => typeof v === "boolean",
  isFunction: (v) => typeof v === "function",
  isNil: (v) => v == null,
  isNull: (v) => v === null,
  isUndefined: (v) => v === undefined,
  isDate: (v) => v instanceof Date,
  isRegExp: (v) => v instanceof RegExp,
  // Collections
  forEach,
  each: forEach,
  map: (c, f) => entriesOf(c).map(([k, v]) => iteratee(f)(v, k, c)),
  filter: (c, f) => entriesOf(c).filter(([k, v]) => iteratee(f)(v, k, c)).map(([, v]) => v),
  reject: (c, f) => entriesOf(c).filter(([k, v]) => !iteratee(f)(v, k, c)).map(([, v]) => v),
  find: (c, f) => (entriesOf(c).find(([k, v]) => iteratee(f)(v, k, c)) || [])[1],
  findIndex: (a, f) => (a || []).findIndex((v, i) => iteratee(f)(v, i, a)),
  findLast: (c, f) => (entriesOf(c).reverse().find(([k, v]) => iteratee(f)(v, k, c)) || [])[1],
  some: (c, f) => entriesOf(c).some(([k, v]) => iteratee(f)(v, k, c)),
  every: (c, f) => entriesOf(c).every(([k, v]) => iteratee(f)(v, k, c)),
  reduce(c, f, acc) {
    const e = entriesOf(c);
    let i = 0;
    if (arguments.length < 3) acc = e[i++][1];
    for (; i < e.length; i++) acc = f(acc, e[i][1], e[i][0], c);
    return acc;
  },
  includes: (c, v) => (typeof c === "string" ? c.indexOf(v) >= 0 : values(c).some((x) => x === v || (x !== x && v !== v))),
  size: (c) => (c == null ? 0 : c.length !== undefined ? c.length : c.size !== undefined ? c.size : Object.keys(c).length),
  groupBy(c, f) {
    const out = {};
    for (const v of values(c)) (out[iteratee(f)(v)] = out[iteratee(f)(v)] || []).push(v);
    return out;
  },
  keyBy: (c, f) => Object.fromEntries(values(c).map((v) => [iteratee(f)(v), v])),
  countBy(c, f) {
    const out = {};
    for (const v of values(c)) out[iteratee(f)(v)] = (out[iteratee(f)(v)] || 0) + 1;
    return out;
  },
  partition: (c, f) => [values(c).filter((v) => iteratee(f)(v)), values(c).filter((v) => !iteratee(f)(v))],
  sortBy: (c, ...fs) => values(c).sort(compareBy((fs.length ? fs.flat() : [(x) => x]).map(iteratee))),
  orderBy: (c, fs, orders) => values(c).sort(compareBy([].concat(fs === undefined ? [(x) => x] : fs).map(iteratee), [].concat(orders || []))),
  shuffle: (c) => values(c).map((v) => [Math.random(), v]).sort((a, b) => a[0] - b[0]).map(([, v]) => v),
  sample: (c) => {
    const v = values(c);
    return v[Math.floor(Math.random() * v.length)];
  },
  // Arrays
  head: (a) => (a || [])[0],
  first: (a) => (a || [])[0],
  last: (a) => (a || [])[(a || []).length - 1],
  tail: (a) => (a || []).slice(1),
  initial: (a) => (a || []).slice(0, -1),
  nth: (a, n) => (a || [])[n < 0 ? a.length + n : n],
  take: (a, n) => (a || []).slice(0, n === undefined ? 1 : n),
  takeRight: (a, n) => (n === 0 ? [] : (a || []).slice(-(n === undefined ? 1 : n))),
  drop: (a, n) => (a || []).slice(n === undefined ? 1 : n),
  dropRight: (a, n) => (a || []).slice(0, Math.max(0, (a || []).length - (n === undefined ? 1 : n))),
  chunk(a, n) {
    const out = [];
    for (let i = 0; i < (a || []).length; i += n || 1) out.push(a.slice(i, i + (n || 1)));
    return out;
  },
  compact: (a) => (a || []).filter(Boolean),
  concat: (...xs) => [].concat(...xs),
  flatten: (a) => flattenDepth(a || [], 1),
  flattenDeep: (a) => flattenDepth(a || [], Infinity),
  flattenDepth: (a, d) => flattenDepth(a || [], d === undefined ? 1 : d),
  uniq: (a) => Array.from(new Set(a || [])),
  uniqBy(a, f) {
    const seen = new Set();
    return (a || []).filter((v) => {
      const k = iteratee(f)(v);
      if (seen.has(k)) return false;
      seen.add(k);
      return true;
    });
  },
  union: (...as) => Array.from(new Set([].concat(...as))),
  intersection: (a, ...rest) => Array.from(new Set(a || [])).filter((v) => rest.every((r) => (r || []).includes(v))),
  difference: (a, ...rest) => (a || []).filter((v) => !rest.flat().includes(v)),
  without: (a, ...vs) => (a || []).filter((v) => !vs.includes(v)),
  xor: (...as) => {
    const all = [].concat(...as.map((a) => Array.from(new Set(a))));
    return all.filter((v) => all.indexOf(v) === all.lastIndexOf(v));
  },
  zip: (...as) => Array.from({ length: Math.max(0, ...as.map((a) => a.length)) }, (_x, i) => as.map((a) => a[i])),
  zipObject: (ks, vs) => Object.fromEntries((ks || []).map((k, i) => [k, (vs || [])[i]])),
  indexOf: (a, v) => (a || []).indexOf(v),
  reverse: (a) => a.reverse(),
  range(start, end, step) {
    if (end === undefined) (end = start), (start = 0);
    step = step === undefined ? (end < start ? -1 : 1) : step;
    const out = [];
    for (let i = start; step > 0 ? i < end : i > end; i += step) out.push(i);
    return out;
  },
  times: (n, f) => Array.from({ length: n }, (_x, i) => (f ? f(i) : i)),
  // Numbers and math
  sum: (a) => (a || []).reduce((s, x) => s + x, 0),
  sumBy: (a, f) => (a || []).reduce((s, x) => s + iteratee(f)(x), 0),
  mean: (a) => (a && a.length ? a.reduce((s, x) => s + x, 0) / a.length : NaN),
  max: (a) => (a && a.length ? a.reduce((m, x) => (x > m ? x : m)) : undefined),
  min: (a) => (a && a.length ? a.reduce((m, x) => (x < m ? x : m)) : undefined),
  maxBy: (a, f) => (a && a.length ? a.reduce((m, x) => (iteratee(f)(x) > iteratee(f)(m) ? x : m)) : undefined),
  minBy: (a, f) => (a && a.length ? a.reduce((m, x) => (iteratee(f)(x) < iteratee(f)(m) ? x : m)) : undefined),
  random(lo, hi, floating) {
    if (hi === undefined || typeof hi === "boolean") (floating = hi), (hi = lo === undefined ? 1 : lo), (lo = 0);
    const r = lo + Math.random() * (hi - lo);
    return floating || !Number.isInteger(lo) || !Number.isInteger(hi) ? r : Math.floor(lo + Math.random() * (hi - lo + 1));
  },
  clamp: (n, lo, hi) => Math.min(Math.max(n, lo), hi),
  inRange: (n, a, b) => (b === undefined ? n >= 0 && n < a : n >= Math.min(a, b) && n < Math.max(a, b)),
  round: (n, p) => Math.round(n * 10 ** (p || 0)) / 10 ** (p || 0),
  toNumber: Number,
  toString: (v) => (v == null ? "" : String(v)),
  toInteger: (v) => Math.trunc(Number(v)) || 0,
  // Strings
  camelCase: (s) => words(s).map((w, i) => (i ? cap(w) : w.toLowerCase())).join(""),
  snakeCase: (s) => words(s).map((w) => w.toLowerCase()).join("_"),
  kebabCase: (s) => words(s).map((w) => w.toLowerCase()).join("-"),
  startCase: (s) => words(s).map((w) => w.charAt(0).toUpperCase() + w.slice(1)).join(" "),
  capitalize: (s) => cap(String(s)),
  upperFirst: (s) => String(s).charAt(0).toUpperCase() + String(s).slice(1),
  lowerFirst: (s) => String(s).charAt(0).toLowerCase() + String(s).slice(1),
  toUpper: (s) => String(s).toUpperCase(),
  toLower: (s) => String(s).toLowerCase(),
  trim: (s, c) => (c === undefined ? String(s).trim() : String(s).replace(new RegExp("^[" + c.replace(/[\]\\^-]/g, "\\$&") + "]+|[" + c.replace(/[\]\\^-]/g, "\\$&") + "]+$", "g"), "")),
  trimStart: (s) => String(s).trimStart(),
  trimEnd: (s) => String(s).trimEnd(),
  pad(s, n, c) {
    s = String(s);
    const total = Math.max(0, n - s.length);
    return s.padStart(s.length + Math.floor(total / 2), c || " ").padEnd(n, c || " ");
  },
  padStart: (s, n, c) => String(s).padStart(n, c === undefined ? " " : c),
  padEnd: (s, n, c) => String(s).padEnd(n, c === undefined ? " " : c),
  repeat: (s, n) => String(s).repeat(n === undefined ? 1 : n),
  startsWith: (s, t, p) => String(s).startsWith(t, p),
  endsWith: (s, t, p) => String(s).endsWith(t, p),
  split: (s, sep, n) => String(s).split(sep, n),
  replace: (s, a, b) => String(s).replace(a, b),
  escapeRegExp: (s) => String(s).replace(/[\\^$.*+?()[\]{}|]/g, "\\$&"),
  escape: (s) => String(s).replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]),
  template: undefined,
  // Functions and utilities
  identity: (x) => x,
  noop: () => {},
  constant: (x) => () => x,
  uniqueId: (() => {
    let n = 0;
    return (prefix) => (prefix || "") + ++n;
  })(),
  once(f) {
    let done = false;
    let r;
    return function () {
      if (!done) (done = true), (r = f.apply(this, arguments));
      return r;
    };
  },
  property: (path) => (o) => get(o, path),
  matches: (src) => (o) => isMatch(o, src),
  iteratee,
};
delete _.template;

module.exports = _;
