// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Unit tests of the built-in modules (crates/rt-script/js/modules) against Node's own
// implementations where Node has one.

import assert from "node:assert/strict";
import { createHash, createHmac, pbkdf2Sync } from "node:crypto";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { MODULES_DIR, quick, runPhase } from "./fake-rt.mjs";

/** Evaluates `expr` in the sandbox (with `C` = crypto-js) and returns its JSON value. */
async function evaluate(expr, prelude = "const C = require('crypto-js');") {
  const out = await runPhase(quick(`${prelude}\nconsole.log(JSON.stringify(${expr}));`));
  assert.equal(out.error, null, out.error);
  return JSON.parse(out.logs[0].text);
}

// Messages across block boundaries, with non-ASCII text.
const MESSAGES = ["", "a", "abc", "ü€😀", "x".repeat(55), "y".repeat(56), "z".repeat(64), "w".repeat(111), "v".repeat(112), "u".repeat(300)];

test("crypto-js hashes match Node's", async () => {
  const algos = { MD5: "md5", SHA1: "sha1", SHA224: "sha224", SHA256: "sha256", SHA384: "sha384", SHA512: "sha512" };
  const got = await evaluate(`${JSON.stringify(MESSAGES)}.map((m) => ${JSON.stringify(Object.keys(algos))}.map((a) => C[a](m).toString()))`);
  MESSAGES.forEach((m, i) => {
    Object.values(algos).forEach((a, k) => assert.equal(got[i][k], createHash(a).update(m, "utf8").digest("hex"), `${a}(${JSON.stringify(m).slice(0, 20)})`));
  });
});

test("crypto-js HMAC matches Node's, also for keys longer than a block", async () => {
  const keys = ["key", "k".repeat(64), "K".repeat(129), "schlüssel"];
  const algos = { HmacMD5: "md5", HmacSHA1: "sha1", HmacSHA256: "sha256", HmacSHA512: "sha512", HmacSHA384: "sha384", HmacSHA224: "sha224" };
  const got = await evaluate(`${JSON.stringify(keys)}.map((k) => ${JSON.stringify(Object.keys(algos))}.map((a) => C[a]("message ü", k).toString(C.enc.Base64)))`);
  keys.forEach((k, i) => {
    Object.values(algos).forEach((a, j) => assert.equal(got[i][j], createHmac(a, k).update("message ü").digest("base64"), `${a} key ${k.length}`));
  });
});

test("crypto-js PBKDF2 matches Node's", async () => {
  const got = await evaluate(`[C.PBKDF2("pw", "salt", { keySize: 8, iterations: 3 }).toString(), C.PBKDF2("pw", C.enc.Hex.parse("0102"), { keySize: 3, iterations: 2, hasher: C.algo.SHA512 }).toString()]`);
  assert.equal(got[0], pbkdf2Sync("pw", "salt", 3, 32, "sha256").toString("hex"));
  assert.equal(got[1], pbkdf2Sync("pw", Buffer.from([1, 2]), 2, 12, "sha512").toString("hex"));
});

test("crypto-js encoders round-trip and match Node's", async () => {
  const text = "Grüße, 世界 😀";
  const got = await evaluate(`[C.enc.Base64.stringify(C.enc.Utf8.parse(${JSON.stringify(text)})), C.enc.Hex.stringify(C.enc.Utf8.parse(${JSON.stringify(text)})), C.enc.Utf8.stringify(C.enc.Base64.parse(${JSON.stringify(Buffer.from(text).toString("base64"))})), C.enc.Base64url.stringify(C.enc.Utf8.parse("??>>")), C.enc.Utf16LE.stringify(C.enc.Utf16LE.parse("ab")), C.lib.WordArray.random(8).sigBytes, C.lib.WordArray.create([0x61626364], 3).toString(C.enc.Latin1)]`);
  assert.equal(got[0], Buffer.from(text).toString("base64"));
  assert.equal(got[1], Buffer.from(text).toString("hex"));
  assert.equal(got[2], text);
  assert.equal(got[3], Buffer.from("??>>").toString("base64url"));
  assert.equal(got[4], "ab");
  assert.equal(got[5], 8);
  assert.equal(got[6], "abc");
});

test("uuid v4 values are distinct and valid", async () => {
  const ids = await evaluate(`Array.from({ length: 200 }, () => uuid.v4())`, "const uuid = require('uuid');");
  assert.equal(new Set(ids).size, 200);
  for (const id of ids) assert.match(id, /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  const parsed = await evaluate(`uuid.stringify(uuid.parse("6ba7b810-9dad-11d1-80b4-00c04fd430c8"))`, "const uuid = require('uuid');");
  assert.equal(parsed, "6ba7b810-9dad-11d1-80b4-00c04fd430c8");
});

test("lodash: iteratee shorthands and edge cases", async () => {
  const got = await evaluate(
    `[_.filter([{ a: { b: 1 } }, { a: { b: 2 } }], ["a.b", 2]).length, _.find([{ x: 1, y: 2 }], { x: 1 }).y, _.map({ a: 1, b: 2 }, (v, k) => k + v), _.reduce([1, 2, 3], (s, x) => s + x), _.isEqual(new Date(1), new Date(1)), _.trim("--a--", "-"), _.random(5, 5), _.times(3, String), _.zipObject(["a", "b"], [1, 2]), _.difference([1, 2, 3], [2]), _.intersection([1, 2], [2, 3]), _.startCase("fooBar baz"), _.mapValues({ a: 1 }, (v) => v * 2), _.size({ a: 1, b: 2 }), _.includes([NaN], NaN), _.pickBy({ a: 1, b: null }, (v) => v !== null), _.max([3, 1, 2]), _.last([1, 2])]`,
    "const _ = require('lodash');",
  );
  assert.deepEqual(got, [1, 2, ["a1", "b2"], 6, true, "a", 5, ["0", "1", "2"], { a: 1, b: 2 }, [1, 3], [2], "Foo Bar Baz", { a: 2 }, 2, true, { a: 1 }, 3, 2]);
});

test("moment: local and UTC agree with Date", async () => {
  const got = await evaluate(
    `[moment(1700000000000).valueOf(), moment.utc([2024, 1, 29]).add(1, "year").format("YYYY-MM-DD"), moment.utc("2024-01-01T00:00:00+02:00").toISOString(), moment.utc("2024-06-15").startOf("week").format("ddd YYYY-MM-DD"), moment.utc("2024-06-15").day(), moment.utc("2020-01-01").diff(moment.utc("2021-01-01"), "years", true), moment.utc("2024-01-01T12:00:00Z").format("hh a ZZ"), moment.utc("2024-03-10T01:02:03Z").add({ hours: 1, minutes: 2 }).format("HH:mm")]`,
    "const moment = require('moment');",
  );
  assert.deepEqual(got, [1700000000000, "2025-02-28", "2023-12-31T22:00:00.000Z", "Sun 2024-06-09", 6, -1, "12 pm +0000", "02:04"]);
});

test("every module file is CommonJS with the license header and loads in the sandbox", async () => {
  const names = readdirSync(MODULES_DIR).filter((f) => f.endsWith(".js")).map((f) => f.slice(0, -3));
  assert.ok(names.length >= 9);
  for (const n of names) {
    assert.match(readFileSync(join(MODULES_DIR, `${n}.js`), "utf8").split("\n").slice(0, 3).join("\n"), /SPDX-License-Identifier: Apache-2\.0/);
    const out = await runPhase(quick(`const m = require(${JSON.stringify(n)}); console.log(typeof m);`));
    assert.equal(out.error, null, `${n}: ${out.error}`);
    assert.ok(["object", "function"].includes(out.logs[0].text), n);
  }
});
