// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('moment')`: the moment.js subset request scripts use, written for RequestTailor (not
// the moment code). English only. Creating (`moment()`, from a Date, milliseconds, an ISO
// string, or a string with a format of the tokens below), `moment.utc`, `moment.unix`,
// `format`, `add`/`subtract`, `startOf`/`endOf`, `diff`, comparisons, getters/setters,
// `toISOString`, `valueOf`, `unix`. Format tokens: YYYY YY M MM MMM MMMM D DD Do d ddd dddd
// H HH h hh m mm s ss S SS SSS A a Z ZZ X x, `[literal]`. Local time is the engine's.

"use strict";

const MONTHS = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
const DAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const UNITS = {
  y: "year", year: "year", years: "year",
  Q: "quarter", quarter: "quarter", quarters: "quarter",
  M: "month", month: "month", months: "month",
  w: "week", week: "week", weeks: "week",
  d: "day", day: "day", days: "day", D: "date", date: "date",
  h: "hour", hour: "hour", hours: "hour",
  m: "minute", minute: "minute", minutes: "minute",
  s: "second", second: "second", seconds: "second",
  ms: "millisecond", millisecond: "millisecond", milliseconds: "millisecond",
};
const MS = { millisecond: 1, second: 1e3, minute: 6e4, hour: 36e5, day: 864e5, week: 6048e5 };

const pad = (n, w) => String(Math.abs(n)).padStart(w, "0");
const unit = (u) => UNITS[u] || UNITS[String(u).toLowerCase()] || (() => { throw new Error("moment: unknown unit " + u); })();

function Moment(ms, utc) {
  this._d = new Date(ms);
  this._utc = !!utc;
}

const P = Moment.prototype;
const get = (m, what) => m._d[(m._utc ? "getUTC" : "get") + what]();
const setter = (m, what, ...v) => m._d[(m._utc ? "setUTC" : "set") + what](...v);

function offsetMinutes(m) {
  return m._utc ? 0 : -m._d.getTimezoneOffset();
}

function zone(m, sep) {
  const o = offsetMinutes(m);
  return (o < 0 ? "-" : "+") + pad(Math.floor(Math.abs(o) / 60), 2) + sep + pad(Math.abs(o) % 60, 2);
}

function ordinal(n) {
  const b = n % 10;
  const s = Math.floor((n % 100) / 10) === 1 ? "th" : b === 1 ? "st" : b === 2 ? "nd" : b === 3 ? "rd" : "th";
  return n + s;
}

const TOKENS = /\[([^\]]*)]|YYYY|YY|MMMM|MMM|MM|M|Do|DD|D|dddd|ddd|d|HH|H|hh|h|mm|m|ss|s|SSS|SS|S|A|a|ZZ|Z|X|x/g;

P.format = function (fmt) {
  if (!this.isValid()) return "Invalid date";
  const f = fmt === undefined ? (this._utc ? "YYYY-MM-DDTHH:mm:ss[Z]" : "YYYY-MM-DDTHH:mm:ssZ") : fmt;
  const y = get(this, "FullYear");
  const M = get(this, "Month");
  const D = get(this, "Date");
  const d = get(this, "Day");
  const H = get(this, "Hours");
  const mi = get(this, "Minutes");
  const s = get(this, "Seconds");
  const ms = get(this, "Milliseconds");
  return f.replace(TOKENS, (t, literal) => {
    if (literal !== undefined) return literal;
    switch (t) {
      case "YYYY": return pad(y, 4);
      case "YY": return pad(y % 100, 2);
      case "MMMM": return MONTHS[M];
      case "MMM": return MONTHS[M].slice(0, 3);
      case "MM": return pad(M + 1, 2);
      case "M": return String(M + 1);
      case "Do": return ordinal(D);
      case "DD": return pad(D, 2);
      case "D": return String(D);
      case "dddd": return DAYS[d];
      case "ddd": return DAYS[d].slice(0, 3);
      case "d": return String(d);
      case "HH": return pad(H, 2);
      case "H": return String(H);
      case "hh": return pad(H % 12 || 12, 2);
      case "h": return String(H % 12 || 12);
      case "mm": return pad(mi, 2);
      case "m": return String(mi);
      case "ss": return pad(s, 2);
      case "s": return String(s);
      case "SSS": return pad(ms, 3);
      case "SS": return pad(Math.floor(ms / 10), 2);
      case "S": return String(Math.floor(ms / 100));
      case "A": return H < 12 ? "AM" : "PM";
      case "a": return H < 12 ? "am" : "pm";
      case "ZZ": return zone(this, "");
      case "Z": return zone(this, ":");
      case "X": return String(Math.floor(this._d.getTime() / 1000));
      case "x": return String(this._d.getTime());
    }
    return t;
  });
};

P.add = function (n, u) {
  if (n !== null && typeof n === "object") {
    for (const k of Object.keys(n)) this.add(n[k], k);
    return this;
  }
  const amount = Number(n);
  const what = unit(u === undefined ? "ms" : u);
  if (what === "year") this.add(amount * 12, "month");
  else if (what === "quarter") this.add(amount * 3, "month");
  else if (what === "month") {
    const day = get(this, "Date");
    setter(this, "Date", 1);
    setter(this, "Month", get(this, "Month") + amount);
    const last = new Date(Date.UTC(get(this, "FullYear"), get(this, "Month") + 1, 0)).getUTCDate();
    setter(this, "Date", Math.min(day, last));
  } else if (what === "date" || what === "day") setter(this, "Date", get(this, "Date") + amount);
  else if (what === "week") setter(this, "Date", get(this, "Date") + 7 * amount);
  else this._d = new Date(this._d.getTime() + amount * MS[what]);
  return this;
};
P.subtract = function (n, u) {
  if (n !== null && typeof n === "object") {
    for (const k of Object.keys(n)) this.add(-n[k], k);
    return this;
  }
  return this.add(-Number(n), u);
};

P.startOf = function (u) {
  const what = unit(u);
  const order = ["year", "quarter", "month", "week", "day", "date", "hour", "minute", "second"];
  const i = order.indexOf(what);
  if (i < 0) return this;
  if (what === "quarter") setter(this, "Month", get(this, "Month") - (get(this, "Month") % 3));
  if (i <= order.indexOf("year")) setter(this, "Month", 0);
  if (what === "week") setter(this, "Date", get(this, "Date") - get(this, "Day"));
  if (i <= order.indexOf("month")) setter(this, "Date", 1);
  if (i <= order.indexOf("date")) setter(this, "Hours", 0);
  if (i <= order.indexOf("hour")) setter(this, "Minutes", 0);
  if (i <= order.indexOf("minute")) setter(this, "Seconds", 0);
  setter(this, "Milliseconds", 0);
  return this;
};
P.endOf = function (u) {
  const what = unit(u);
  return this.startOf(what).add(1, what === "date" ? "day" : what).subtract(1, "ms");
};

P.diff = function (other, u, float) {
  const o = toMoment(other);
  const what = u === undefined ? "millisecond" : unit(u);
  let r;
  if (what === "year" || what === "month" || what === "quarter") {
    const months = (get(this, "FullYear") - get(o, "FullYear")) * 12 + (get(this, "Month") - get(o, "Month"));
    const anchor = o.clone().add(months, "month");
    const frac = (this._d - anchor._d) / Math.abs(anchor.clone().add(this._d - anchor._d < 0 ? -1 : 1, "month")._d - anchor._d);
    const total = months + (isFinite(frac) ? frac : 0);
    r = what === "year" ? total / 12 : what === "quarter" ? total / 3 : total;
  } else {
    r = (this._d.getTime() - o._d.getTime() - (what === "day" || what === "week" ? (offsetMinutes(this) - offsetMinutes(o)) * -6e4 : 0)) / MS[what === "date" ? "day" : what];
  }
  return float ? r : r < 0 ? Math.ceil(r) : Math.floor(r);
};

P.isBefore = function (o, u) {
  return u ? this.clone().endOf(u)._d < toMoment(o)._d : this._d < toMoment(o)._d;
};
P.isAfter = function (o, u) {
  return u ? this.clone().startOf(u)._d > toMoment(o)._d : this._d > toMoment(o)._d;
};
P.isSame = function (o, u) {
  if (!u) return this._d.getTime() === toMoment(o)._d.getTime();
  const t = toMoment(o)._d.getTime();
  return this.clone().startOf(u)._d.getTime() <= t && t <= this.clone().endOf(u)._d.getTime();
};
P.isSameOrBefore = function (o, u) {
  return this.isSame(o, u) || this.isBefore(o, u);
};
P.isSameOrAfter = function (o, u) {
  return this.isSame(o, u) || this.isAfter(o, u);
};
P.isBetween = function (a, b, u) {
  return this.isAfter(a, u) && this.isBefore(b, u);
};
P.isValid = function () {
  return !isNaN(this._d.getTime());
};
P.clone = function () {
  return new Moment(this._d.getTime(), this._utc);
};
P.utc = function () {
  this._utc = true;
  return this;
};
P.local = function () {
  this._utc = false;
  return this;
};
P.isUTC = function () {
  return this._utc;
};
P.utcOffset = function () {
  return offsetMinutes(this);
};
P.toDate = function () {
  return new Date(this._d.getTime());
};
P.toISOString = function () {
  return this._d.toISOString();
};
P.toJSON = P.toISOString;
P.toString = function () {
  return this.format("ddd MMM DD YYYY HH:mm:ss [GMT]ZZ");
};
P.valueOf = function () {
  return this._d.getTime();
};
P.unix = function () {
  return Math.floor(this._d.getTime() / 1000);
};
P.daysInMonth = function () {
  return new Date(Date.UTC(get(this, "FullYear"), get(this, "Month") + 1, 0)).getUTCDate();
};
P.get = function (u) {
  return this[unit(u)]();
};
P.set = function (u, v) {
  return this[unit(u)](v);
};

for (const [name, what] of [["year", "FullYear"], ["month", "Month"], ["date", "Date"], ["hour", "Hours"], ["minute", "Minutes"], ["second", "Seconds"], ["millisecond", "Milliseconds"]]) {
  P[name] = P[name + "s"] = function (v) {
    if (v === undefined) return get(this, what);
    setter(this, what, v);
    return this;
  };
}
P.day = P.days = function (v) {
  if (v === undefined) return get(this, "Day");
  return this.add(v - get(this, "Day"), "day");
};

/** A moment from a string with `format` (numeric tokens and MMM/MMMM). */
function parseFormat(str, fmt, utc) {
  const parts = { Y: 1970, M: 0, D: 1, H: 0, m: 0, s: 0, ms: 0, pm: undefined, offset: undefined };
  let re = "";
  const fields = [];
  fmt.replace(/\[([^\]]*)]|YYYY|YY|MMMM|MMM|MM|M|DD|D|HH|H|hh|h|mm|m|ss|s|SSS|A|a|ZZ|Z|X|x|([\s\S])/g, (t, lit, ch) => {
    if (lit !== undefined) re += lit.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    else if (ch !== undefined) re += ch.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
    else {
      fields.push(t);
      re += { YYYY: "(\\d{4})", YY: "(\\d{2})", MMMM: "([A-Za-z]+)", MMM: "([A-Za-z]{3})", A: "(AM|PM|am|pm)", a: "(AM|PM|am|pm)", Z: "([+-]\\d{2}:?\\d{2}|Z)", ZZ: "([+-]\\d{2}:?\\d{2}|Z)", X: "(\\d+)", x: "(\\d+)", SSS: "(\\d{1,3})" }[t] || "(\\d{1,2})";
    }
    return t;
  });
  const m = new RegExp("^" + re).exec(str);
  if (!m) return new Moment(NaN, utc);
  fields.forEach((f, i) => {
    const v = m[i + 1];
    switch (f) {
      case "YYYY": parts.Y = +v; break;
      case "YY": parts.Y = 2000 + +v - (+v > 68 ? 100 : 0); break;
      case "MMMM": case "MMM": parts.M = MONTHS.findIndex((x) => x.toLowerCase().startsWith(v.toLowerCase())); break;
      case "MM": case "M": parts.M = +v - 1; break;
      case "DD": case "D": parts.D = +v; break;
      case "HH": case "H": case "hh": case "h": parts.H = +v; break;
      case "mm": case "m": parts.m = +v; break;
      case "ss": case "s": parts.s = +v; break;
      case "SSS": parts.ms = +v.padEnd(3, "0"); break;
      case "A": case "a": parts.pm = v.toLowerCase() === "pm"; break;
      case "Z": case "ZZ": parts.offset = v === "Z" ? 0 : (v[0] === "-" ? -1 : 1) * (+v.slice(1, 3) * 60 + +v.slice(-2)); break;
      case "X": parts.epoch = +v * 1000; break;
      case "x": parts.epoch = +v; break;
    }
  });
  if (parts.epoch !== undefined) return new Moment(parts.epoch, utc);
  if (parts.pm !== undefined) parts.H = (parts.H % 12) + (parts.pm ? 12 : 0);
  if (parts.offset !== undefined) return new Moment(Date.UTC(parts.Y, parts.M, parts.D, parts.H, parts.m, parts.s, parts.ms) - parts.offset * 6e4, utc);
  const ms = utc ? Date.UTC(parts.Y, parts.M, parts.D, parts.H, parts.m, parts.s, parts.ms) : new Date(parts.Y, parts.M, parts.D, parts.H, parts.m, parts.s, parts.ms).getTime();
  return new Moment(ms, utc);
}

function create(input, fmt, utc) {
  if (input === undefined || input === null) return new Moment(Date.now(), utc);
  if (input instanceof Moment) return new Moment(input._d.getTime(), utc || input._utc);
  if (input instanceof Date) return new Moment(input.getTime(), utc);
  if (typeof input === "number") return new Moment(input, utc);
  if (typeof input === "string") {
    if (fmt) return parseFormat(input, fmt, utc);
    const dateOnly = /^(\d{4})-(\d{2})-(\d{2})$/.exec(input);
    if (dateOnly) return parseFormat(input, "YYYY-MM-DD", utc);
    const noZone = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}(:\d{2}(\.\d+)?)?$/.test(input);
    if (noZone) return parseFormat(input.replace(" ", "T"), "YYYY-MM-DDTHH:mm:ss.SSS".slice(0, input.length >= 19 ? (input.length > 19 ? 23 : 19) : 16), utc);
    return new Moment(Date.parse(input), utc);
  }
  if (Array.isArray(input)) {
    const [Y, M = 0, D = 1, H = 0, mi = 0, s = 0, ms = 0] = input;
    return new Moment(utc ? Date.UTC(Y, M, D, H, mi, s, ms) : new Date(Y, M, D, H, mi, s, ms).getTime(), utc);
  }
  return new Moment(NaN, utc);
}

function toMoment(v) {
  return v instanceof Moment ? v : create(v);
}

function moment(input, fmt) {
  return create(input, fmt, false);
}
moment.utc = (input, fmt) => create(input, fmt, true);
moment.unix = (s) => new Moment(s * 1000, false);
moment.isMoment = (v) => v instanceof Moment;
moment.isDate = (v) => v instanceof Date;
moment.now = () => Date.now();
moment.ISO_8601 = undefined;
moment.fn = P;

module.exports = moment;
