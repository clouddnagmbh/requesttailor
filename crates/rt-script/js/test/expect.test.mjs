// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Unit tests of shims/00-expect.js: chai's messages, flags, chainable words, response
// assertions and the JSON Schema validator.

import assert from "node:assert/strict";
import { test } from "node:test";

import { quick, runPhase } from "./fake-rt.mjs";

/** Runs `code` as one test and returns its error message (`null` if it passed). */
async function failure(code, extra) {
  const out = await runPhase(quick(`test("t", () => { ${code} });`, extra));
  assert.equal(out.error, null, out.error);
  assert.equal(out.tests.length, 1);
  return out.tests[0].passed ? null : out.tests[0].error;
}

const MESSAGES = [
  ["expect(1).to.equal(2)", "expected 1 to equal 2"],
  ["expect(1).to.not.equal(1)", "expected 1 to not equal 1"],
  ["expect([1]).to.deep.equal([2])", "expected [ 1 ] to deeply equal [ 2 ]"],
  ["expect({ a: 1 }).to.not.eql({ a: 1 })", "expected { a: 1 } to not deeply equal { a: 1 }"],
  ["expect(null).to.exist", "expected null to exist"],
  ["expect(1).to.not.exist", "expected 1 to not exist"],
  ["expect(undefined).to.be.ok", "expected undefined to be truthy"],
  ["expect(1).to.not.be.ok", "expected 1 to be falsy"],
  ["expect('a').to.be.true", "expected 'a' to be true"],
  ["expect(1).to.be.null", "expected 1 to be null"],
  ["expect(1).to.be.undefined", "expected 1 to be undefined"],
  ["expect([1]).to.be.empty", "expected [ 1 ] to be empty"],
  ["expect([]).to.not.be.empty", "expected [] not to be empty"],
  ["expect(1).to.be.below(1)", "expected 1 to be below 1"],
  ["expect(1).to.be.at.least(2)", "expected 1 to be at least 2"],
  ["expect(3).to.be.at.most(2)", "expected 3 to be at most 2"],
  ["expect(3).to.not.be.above(2)", "expected 3 to be at most 2"],
  ["expect(11).to.be.within(1, 10)", "expected 11 to be within 1..10"],
  ["expect('x').to.be.above(1)", "expected 'x' to be a number or a date"],
  ["expect([1, 2]).to.have.length.below(1)", "expected [ 1, 2 ] to have a length below 1 but got 2"],
  ["expect([]).to.be.an('object')", "expected [] to be an object"],
  ["expect('abc').to.include('z')", "expected 'abc' to include 'z'"],
  ["expect({ a: 1 }).to.include({ a: 2 })", "expected { a: 1 } to include { a: 2 }"],
  ["expect([{ a: 1 }]).to.include({ a: 1 })", "expected [ { a: 1 } ] to include { a: 1 }"],
  ["expect([{ a: 1 }]).to.not.deep.include({ a: 1 })", "expected [ { a: 1 } ] to not deep include { a: 1 }"],
  ["expect({ a: 1 }).to.have.property('b')", "expected { a: 1 } to have property 'b'"],
  ["expect({ a: 1 }).to.not.have.property('a')", "expected { a: 1 } to not have property 'a'"],
  ["expect({ a: { b: 1 } }).to.have.nested.property('a.c')", "expected { a: { b: 1 } } to have nested property 'a.c'"],
  ["expect({ a: 1 }).to.have.deep.property('a', 2)", "expected { a: 1 } to have deep property 'a' of 2, but got 1"],
  ["expect('ab').to.have.lengthOf(3)", "expected 'ab' to have a length of 3 but got 2"],
  ["expect('abc').to.match(/x/)", "expected 'abc' to match /x/"],
  ["expect('abc').to.not.match(/a/)", "expected 'abc' not to match /a/"],
  ["expect('abc').to.have.string('x')", "expected 'abc' to contain 'x'"],
  ["expect({ a: 1, b: 2 }).to.have.any.keys('x', 'y')", "expected { a: 1, b: 2 } to have keys 'x', or 'y'"],
  ["expect({ a: 1 }).to.have.key('b')", "expected { a: 1 } to have key 'b'"],
  ["expect([1, 2]).to.have.members([1])", "expected [ 1, 2 ] to have the same members as [ 1 ]"],
  ["expect([1, 2]).to.include.members([3])", "expected [ 1, 2 ] to be a superset of [ 3 ]"],
  ["expect(1).to.be.oneOf([2, 3])", "expected 1 to be one of [ 2, 3 ]"],
  ["expect({}).to.be.instanceOf(Array)", "expected {} to be an instance of Array"],
  ["expect(() => { throw new Error('a'); }).to.throw('b')", "expected [Function] to throw error including 'b' but got 'a'"],
  ["expect(() => { throw new Error('a'); }).to.not.throw()", "to not throw an error but [Error: a] was thrown"],
  ["expect(3).to.satisfy((n) => n > 5)", "expected 3 to satisfy [Function]"],
  ["expect(1.5).to.be.closeTo(1, 0.1)", "expected 1.5 to be close to 1 +/- 0.1"],
  ["expect(1).to.be.NaN", "expected 1 to be NaN"],
  ["expect(1, 'the count').to.equal(2)", "the count: expected 1 to equal 2"],
  ["expect.fail('boom')", "boom"],
];

for (const [code, message] of MESSAGES) {
  test(`message: ${code}`, async () => {
    const error = await failure(code);
    assert.ok(error !== null, "expected a failure");
    assert.ok(error.includes(message), `${error}\n  does not include\n${message}`);
  });
}

test("chain words have no effect and flags reach the next assertion only", async () => {
  assert.equal(await failure("expect(1).to.be.been.is.that.which.and.has.have.with.at.of.same.but.does.still.equal(1)"), null);
  assert.equal(await failure("expect([1, 2]).to.not.include(3).and.include(1)"), "expected [ 1, 2 ] to not include 1");
  assert.equal(await failure("expect({ a: 1 }).to.have.property('a').that.is.a('number').and.equals(1)"), null);
  assert.equal(await failure("expect([1, 2, 3]).to.have.lengthOf.at.least(2)"), null);
  assert.equal(await failure("expect([1, 2]).to.include.members([1]).and.have.lengthOf(2)"), null);
});

test("inspect: strings, numbers, nested values, special objects", async () => {
  assert.equal(await failure("expect(-0).to.equal(1)"), "expected -0 to equal 1");
  assert.equal(await failure("expect({ 'a-b': 'it\\'s' }).to.equal(1)"), "expected { 'a-b': 'it\\'s' } to equal 1");
  assert.equal(await failure("expect(new Date(0)).to.equal(1)"), "expected 1970-01-01T00:00:00.000Z to equal 1");
  assert.equal(await failure("expect(/x/g).to.equal(1)"), "expected /x/g to equal 1");
  assert.equal(await failure("expect(function named() {}).to.equal(1)"), "expected [Function named] to equal 1");
  assert.equal(await failure("expect(new Map([[1, 2]])).to.equal(1)"), "expected Map{ 1 => 2 } to equal 1");
  assert.equal(await failure("const o = {}; o.self = o; expect(o).to.equal(1)"), "expected { self: [Circular] } to equal 1");
});

test("deep equality follows chai: types, NaN, -0, dates, sets", async () => {
  assert.equal(await failure("expect(NaN).to.eql(NaN)"), null);
  assert.ok(await failure("expect(-0).to.eql(0)"));
  assert.ok(await failure("expect([1]).to.eql({ 0: 1 })"));
  assert.equal(await failure("expect(new Date(5)).to.eql(new Date(5))"), null);
  assert.equal(await failure("expect(new Set([1, { a: 1 }])).to.eql(new Set([{ a: 1 }, 1]))"), null);
  assert.ok(await failure("expect({ a: undefined }).to.eql({})"));
});

test("response assertions need a response", async () => {
  const error = await failure("expect(5).to.have.status(200)");
  assert.match(error, /needs a response/);
});

test("on a plain value, ok is truthiness; on a response, the status class", async () => {
  assert.equal(await failure("expect({ status: 500 }).to.be.ok"), null);
  assert.equal(
    await failure("expect(rt.response).to.be.ok", { response: { status: 500, headers: [], body: "", time_ms: 1, url: "" } }),
    "expected response code to be 2XX but found 500",
  );
});

test("header values compare case-insensitively, missing headers say so", async () => {
  assert.equal(await failure("expect(rt.response).to.have.header('content-type', 'APPLICATION/JSON')"), null);
  assert.equal(await failure("expect(rt.response).to.have.header('content-type', 'text/plain')"), "expected 'content-type' response header to be 'text/plain' but got 'application/json'");
  assert.equal(await failure("expect(rt.response).to.not.have.header('content-type', 'text/plain')"), null);
});

test("response body assertions", async () => {
  const response = { status: 200, headers: [], body: "plain text", time_ms: 1, url: "" };
  assert.equal(await failure("expect(rt.response).to.have.body('plain text')", { response }), null);
  assert.equal(await failure("expect(rt.response).to.have.body(/plain/)", { response }), null);
  assert.match(await failure("expect(rt.response).to.have.jsonBody()", { response }), /to be a valid json/);
  assert.match(await failure("expect(rt.response).to.be.json", { response }), /to be a valid json/);
  assert.equal(await failure("expect(rt.response).to.have.body({ id: 7 })"), null);
});

test("JSON Schema keywords", async () => {
  const cases = [
    [{ type: "string", minLength: 2 }, "a", "should NOT be shorter than 2 characters"],
    [{ type: "string", pattern: "^a" }, "b", 'should match pattern "^a"'],
    [{ type: "number", minimum: 1 }, 0, "should be >= 1"],
    [{ type: "number", exclusiveMaximum: 5 }, 5, "should be < 5"],
    [{ type: "array", minItems: 1 }, [], "should NOT have fewer than 1 items"],
    [{ type: "array", uniqueItems: true }, [1, 1], "duplicate items"],
    [{ items: [{ type: "string" }], additionalItems: false }, ["a", 1], "data[1] should not exist"],
    [{ anyOf: [{ type: "string" }, { type: "number" }] }, null, "should match some schema in anyOf"],
    [{ oneOf: [{ type: "number" }, { type: "integer" }] }, 1, "should match exactly one schema in oneOf"],
    [{ not: { type: "null" } }, null, "should NOT be valid"],
    [{ const: 3 }, 4, "should be equal to constant"],
    [{ type: ["string", "null"] }, 1, "should be string,null"],
    [{ properties: { a: { $ref: "#/$defs/s" } }, $defs: { s: { type: "string" } } }, { a: 1 }, "data.a should be string"],
    [{ patternProperties: { "^x-": { type: "number" } } }, { "x-a": "1" }, "data['x-a'] should be number"],
    [{ type: "object", properties: { n: { type: "string", nullable: true } } }, { n: null }, null],
    [{ if: { properties: { k: { const: "a" } } }, then: { required: ["v"] } }, { k: "a" }, "should have required property 'v'"],
  ];
  for (const [schema, data, message] of cases) {
    const error = await failure(`expect(${JSON.stringify(data)}).to.have.jsonSchema(${JSON.stringify(schema)})`);
    if (message === null) assert.equal(error, null, JSON.stringify(schema));
    else assert.ok(error && error.includes(message), `${JSON.stringify(schema)}: ${error}`);
  }
});

test("a remote $ref is refused with a clear error", async () => {
  const error = await failure("expect(1).to.have.jsonSchema({ $ref: 'http://x/s.json' })");
  assert.match(error, /only local \$ref/);
});

test("user scripts may redeclare the globals the shims define", async () => {
  const out = await runPhase(quick("const res = 1; let pm = 2; const expect = (x) => x; console.log(String(res + pm + expect(3)));"));
  assert.equal(out.error, null);
  assert.equal(out.logs[0].text, "6");
});
