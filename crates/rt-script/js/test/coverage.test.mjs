// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The coverage list (crates/rt-format/src/import/shim-apis.txt) against the shims, and the
// API scan of scripts/shim-frequency.mjs.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import { apiUses, COVERAGE_DOC, coverageList, coverageOf, DOC_END, DOC_START, frequencyMarkdown } from "../../../../scripts/shim-frequency.mjs";
import { quick, runPhase } from "./fake-rt.mjs";

/** JavaScript that is truthy when `api` exists once the shims are loaded. */
function probe(api) {
  let m;
  if ((m = /^require\('(.+)'\)$/.exec(api))) return `(() => { try { return !!require(${JSON.stringify(m[1])}); } catch { return false; } })()`;
  if ((m = /^expect\(\)\.(\w+)$/.exec(api))) return `(${JSON.stringify(m[1])} in expect(1)) || typeof expect[${JSON.stringify(m[1])}] === "function"`;
  if ((m = /^(\w+)\.response\.to\.(\w+)$/.exec(api))) return `${JSON.stringify(m[2])} in ${m[1]}.response.to.have`;
  if ((m = /^([\w$]+)\(\)$/.exec(api))) return `typeof ${m[1]} === "function"`;
  if ((m = /^([\w$]+)\[\]$/.exec(api))) return `typeof ${m[1]} === "object"`;
  const [root, ...rest] = api.split(".");
  return `(() => { let v = typeof ${root} === "undefined" ? undefined : ${root}; const path = ${JSON.stringify(rest)}; for (const k of path.slice(0, -1)) { if (v === undefined || v === null) return false; v = v[k]; } return path.length ? v !== undefined && v !== null && path.at(-1) in Object(v) : v !== undefined; })()`;
}

test("every supported and partial API of the coverage list exists", async () => {
  const list = coverageList();
  assert.ok(list.size > 300, `only ${list.size} entries`);
  const apis = [...list].filter(([, e]) => e.status !== "unsupported").map(([api]) => api);
  const code = `console.log(JSON.stringify([${apis.map(probe).join(",\n")}]));`;
  const out = await runPhase(quick(code));
  assert.equal(out.error, null, out.error);
  const found = JSON.parse(out.logs[0].text);
  const missing = apis.filter((_, i) => !found[i]);
  assert.deepEqual(missing, []);
});

test("every unsupported API that exists as a stub throws a pointer to SHIM-COVERAGE.md", async () => {
  const list = coverageList();
  const stubs = ["bru.runRequest", "bru.cwd", "bru.getTestResults", "req.onFail", "req.getPathParams", "pm.vault.get", "pm.require", "pm.environment.clear", "xml2Json()", "insomnia.parentFolders.get"];
  for (const api of stubs) {
    assert.equal(coverageOf(api, list)?.status, "unsupported", api);
    const out = await runPhase(quick(`${api.replace(/\(\)$/, "")}("x");`));
    assert.match(out.error ?? "", /not supported in RequestTailor.*SHIM-COVERAGE\.md/, api);
  }
});

test("the coverage list: entries are well-formed and unique", () => {
  const list = coverageList();
  for (const [api, e] of list) {
    assert.ok(["supported", "partial", "unsupported"].includes(e.status), api);
    if (e.status === "partial") assert.ok(e.note, `${api}: a partial entry needs a note`);
  }
});

test("coverageOf: longest prefix, insomnia falls back to pm, unknown modules", () => {
  const list = coverageList();
  assert.equal(coverageOf("res.body.items", list).api, "res.body");
  assert.equal(coverageOf("pm.environment.clear", list).status, "unsupported");
  assert.equal(coverageOf("insomnia.environment.get", list).api, "insomnia.environment.get");
  assert.equal(coverageOf("insomnia.environment.get", list).status, "supported");
  assert.equal(coverageOf("insomnia.parentFolders.get", list).status, "unsupported");
  assert.equal(coverageOf("require('fs')", list).status, "unsupported");
  assert.equal(coverageOf("require('crypto-js')", list).status, "partial");
  assert.equal(coverageOf("nothing.at.all", list), undefined);
});

test("apiUses: member paths, calls, expect words, strings and comments are skipped", () => {
  const uses = (code, dialect) => apiUses(code, dialect).map((u) => u.api);
  assert.deepEqual(uses(`pm.environment.set("a", 1); // pm.globals.get("x")\nconst s = "pm.vault.get()";`, "postman"), ["pm.environment.set"]);
  assert.deepEqual(uses(`pm.expect(x).to.not.have.property("a");`, "postman"), ["pm.expect", "expect().not", "expect().property"]);
  assert.deepEqual(uses(`pm.response.to.have.status(200);`, "postman"), ["pm.response.to.status"]);
  assert.deepEqual(uses(`const id = res("data.id"); test("t", () => expect(res.getStatus()).to.equal(200));`, "bruno"), ["res()", "test()", "expect().equal", "res.getStatus"]);
  assert.deepEqual(uses(`tests["ok"] = responseCode.code === 200;`, "postman"), ["tests[]", "responseCode.code"]);
  assert.deepEqual(uses(`const C = require('crypto-js'); const u = require("uuid");`, "bruno"), ["require('crypto-js')", "require('uuid')"]);
  assert.deepEqual(uses(`const x = \`\${bru.getVar("a")}\`;`, "bruno"), ["bru.getVar"]);
});

test("SHIM-COVERAGE.md has the current frequency table", () => {
  const doc = readFileSync(COVERAGE_DOC, "utf8");
  const table = doc.slice(doc.indexOf(DOC_START) + DOC_START.length, doc.indexOf(DOC_END)).trim();
  assert.ok(table === frequencyMarkdown().trim(), "out of date: run node scripts/shim-frequency.mjs --update-doc");
});

test("no corpus API is missing from the coverage list", () => {
  const unlisted = frequencyMarkdown().split("\n").filter((l) => l.includes("| unlisted |"));
  assert.deepEqual(unlisted, []);
});
