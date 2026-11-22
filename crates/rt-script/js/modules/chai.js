// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// `require('chai')`: `expect` (the global one, see shims/00-expect.js), `AssertionError`, and
// the `assert` functions request scripts use. `should` and plugins (`chai.use`) are not
// included.

"use strict";

const S = globalThis.__rtShim;
const { AssertionError, deepEqual, inspect } = S;

function fail(message, fallback) {
  throw new AssertionError(message === undefined ? fallback : message);
}

function assert(value, message) {
  if (!value) fail(message, "expected " + inspect(value) + " to be truthy");
}

Object.assign(assert, {
  ok: assert,
  isOk: assert,
  notOk: (v, m) => v && fail(m, "expected " + inspect(v) + " to be falsy"),
  isNotOk: (v, m) => v && fail(m, "expected " + inspect(v) + " to be falsy"),
  fail: (m) => fail(m, "assert.fail()"),
  equal: (a, b, m) => a != b && fail(m, "expected " + inspect(a) + " to equal " + inspect(b)),
  notEqual: (a, b, m) => a == b && fail(m, "expected " + inspect(a) + " to not equal " + inspect(b)),
  strictEqual: (a, b, m) => a !== b && fail(m, "expected " + inspect(a) + " to equal " + inspect(b)),
  notStrictEqual: (a, b, m) => a === b && fail(m, "expected " + inspect(a) + " to not equal " + inspect(b)),
  deepEqual: (a, b, m) => !deepEqual(a, b) && fail(m, "expected " + inspect(a) + " to deeply equal " + inspect(b)),
  notDeepEqual: (a, b, m) => deepEqual(a, b) && fail(m, "expected " + inspect(a) + " to not deeply equal " + inspect(b)),
  isTrue: (v, m) => v !== true && fail(m, "expected " + inspect(v) + " to be true"),
  isFalse: (v, m) => v !== false && fail(m, "expected " + inspect(v) + " to be false"),
  isNull: (v, m) => v !== null && fail(m, "expected " + inspect(v) + " to be null"),
  isNotNull: (v, m) => v === null && fail(m, "expected " + inspect(v) + " not to be null"),
  isUndefined: (v, m) => v !== undefined && fail(m, "expected " + inspect(v) + " to be undefined"),
  isDefined: (v, m) => v === undefined && fail(m, "expected " + inspect(v) + " to not equal undefined"),
  exists: (v, m) => v == null && fail(m, "expected " + inspect(v) + " to exist"),
  notExists: (v, m) => v != null && fail(m, "expected " + inspect(v) + " to not exist"),
  isString: (v, m) => typeof v !== "string" && fail(m, "expected " + inspect(v) + " to be a string"),
  isNumber: (v, m) => typeof v !== "number" && fail(m, "expected " + inspect(v) + " to be a number"),
  isBoolean: (v, m) => typeof v !== "boolean" && fail(m, "expected " + inspect(v) + " to be a boolean"),
  isArray: (v, m) => !Array.isArray(v) && fail(m, "expected " + inspect(v) + " to be an array"),
  isObject: (v, m) => S.typeOf(v) !== "object" && fail(m, "expected " + inspect(v) + " to be an object"),
  isFunction: (v, m) => typeof v !== "function" && fail(m, "expected " + inspect(v) + " to be a function"),
  typeOf: (v, t, m) => S.typeOf(v) !== t && fail(m, "expected " + inspect(v) + " to be a" + (/^[aeiou]/.test(t) ? "n " : " ") + t),
  include: (h, n, m) => expect(h, m).to.include(n),
  notInclude: (h, n, m) => expect(h, m).to.not.include(n),
  match: (v, re, m) => expect(v, m).to.match(re),
  property: (o, p, m) => expect(o, m).to.have.property(p),
  propertyVal: (o, p, v, m) => expect(o, m).to.have.property(p, v),
  lengthOf: (o, n, m) => expect(o, m).to.have.lengthOf(n),
  isAbove: (a, b, m) => expect(a, m).to.be.above(b),
  isBelow: (a, b, m) => expect(a, m).to.be.below(b),
  isAtLeast: (a, b, m) => expect(a, m).to.be.at.least(b),
  isAtMost: (a, b, m) => expect(a, m).to.be.at.most(b),
  oneOf: (v, l, m) => expect(v, m).to.be.oneOf(l),
  sameMembers: (a, b, m) => expect(a, m).to.have.same.members(b),
  hasAllKeys: (o, k, m) => expect(o, m).to.have.all.keys(k),
  throws: (f, e, msg, m) => expect(f, m).to.throw(e, msg),
  doesNotThrow: (f, e, msg, m) => expect(f, m).to.not.throw(e, msg),
  isEmpty: (v, m) => expect(v, m).to.be.empty,
  isNotEmpty: (v, m) => expect(v, m).to.not.be.empty,
  instanceOf: (v, c, m) => expect(v, m).to.be.instanceOf(c),
  jsonSchema: (v, s, m) => expect(v, m).to.have.jsonSchema(s),
});

module.exports = {
  expect: (v, m) => expect(v, m),
  assert,
  AssertionError,
  Assertion: S.Assertion,
  use() {
    throw new Error("chai.use is not supported in RequestTailor; see SHIM-COVERAGE.md");
  },
};
