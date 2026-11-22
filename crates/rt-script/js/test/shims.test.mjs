// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Unit tests of the Bruno and Postman shims beyond the case files: what `sendRequest` sends,
// read-only requests after the response, typed variables, list helpers.

import assert from "node:assert/strict";
import { test } from "node:test";

import { quick, runPhase } from "./fake-rt.mjs";

const before = (code, extra = {}) => runPhase(quick(code, { phase: "before-request", ...extra }));
const ok = (url, body = "{}", headers = [["Content-Type", "application/json"]]) => ({ url, response: { status: 200, headers, body, time_ms: 1, url } });

test("bru.sendRequest sends data as JSON, params in the URL, headers as given", async () => {
  const out = await before(`await bru.sendRequest({ url: "http://a/b", method: "post", headers: { "X-A": "1" }, params: { q: "x y" }, data: { n: 1 } });`, {
    send: [ok("http://a/b?q=x%20y")],
  });
  assert.equal(out.error, null);
  assert.deepEqual(JSON.parse(JSON.stringify(out.sent)), [{ method: "POST", url: "http://a/b?q=x%20y", headers: [["X-A", "1"], ["Content-Type", "application/json"]], body: '{"n":1}' }]);
});

test("bru.sendRequest hands a failure to the callback", async () => {
  const out = await before(`await bru.sendRequest({ url: "http://nowhere/" }, (err, res) => { console.log(String(res), err.message); });`);
  assert.equal(out.error, null);
  assert.deepEqual(out.logs.map((l) => l.text), ["null no canned response for http://nowhere/"]);
});

test("pm.sendRequest: header arrays, raw bodies, graphql, a string URL", async () => {
  const out = await before(
    `await pm.sendRequest({ url: "http://a/1", method: "PUT", header: [{ key: "A", value: "1" }, { key: "B", value: "2", disabled: true }, "C: 3"], body: { mode: "raw", raw: "text" } });
await pm.sendRequest({ url: "http://a/2", method: "POST", body: { mode: "graphql", graphql: { query: "{ x }" } } });
await pm.sendRequest("http://a/3");
await pm.sendRequest({ url: { raw: "http://a/4" }, header: { D: 4 } });`,
    { send: [ok("http://a/1"), ok("http://a/2"), ok("http://a/3"), ok("http://a/4")] },
  );
  assert.equal(out.error, null);
  assert.deepEqual(JSON.parse(JSON.stringify(out.sent)), [
    { method: "PUT", url: "http://a/1", headers: [["A", "1"], ["C", "3"]], body: "text" },
    { method: "POST", url: "http://a/2", headers: [["Content-Type", "application/json"]], body: '{"query":"{ x }"}' },
    { method: "GET", url: "http://a/3", headers: [], body: null },
    { method: "GET", url: "http://a/4", headers: [["D", "4"]], body: null },
  ]);
});

test("pm.sendRequest: a failing request calls back with the error, the promise rejects", async () => {
  const cb = await before(`pm.sendRequest("http://nowhere/", (err, res) => console.log(err.message, String(res)));`);
  assert.equal(cb.error, null);
  assert.equal(cb.logs[0].text, "no canned response for http://nowhere/ null");
  const p = await before(`try { await pm.sendRequest("http://nowhere/"); } catch (e) { console.log("caught", e.message); }`);
  assert.equal(p.logs[0].text, "caught no canned response for http://nowhere/");
  const form = await before(`await pm.sendRequest({ url: "http://a/", body: { mode: "formdata", formdata: [] } });`);
  assert.match(form.error, /body mode 'formdata' is not supported/);
});

test("the request is read-only after the response", async () => {
  const out = await runPhase(quick(`pm.request.headers.add({ key: "A", value: "1" });`));
  assert.match(out.error, /read-only/);
  const bru = await runPhase(quick(`req.setHeader("A", "1");`));
  assert.match(bru.error, /read-only/);
});

test("typed values: unset in this phase wins over lower layers", async () => {
  const out = await before(`bru.setEnvVar("x", 5);
console.log(typeof bru.getEnvVar("x"));
bru.deleteEnvVar("x");
console.log(String(bru.getEnvVar("x")), bru.getVar("x"));`, {
    variables: [{ name: "x", value: "from-collection", source: { layer: "collection" }, secret: false }],
  });
  assert.deepEqual(out.logs.map((l) => l.text), ["number", "undefined from-collection"]);
});

test("setting undefined unsets", async () => {
  const out = await before(`pm.environment.set("a", undefined);`);
  assert.deepEqual(out.var_changes, [{ name: "a", value: null, scope: "environment" }]);
});

test("bru.setHeaders replaces every header, req properties mirror the getters", async () => {
  const out = await before(`req.setHeaders({ A: "1", B: 2 });
req.url = "http://new";
req.method = "patch";
req.body = { x: 1 };
console.log(JSON.stringify(req.headers), req.url, req.method, JSON.stringify(req.body));`, {
    request: { method: "GET", url: "http://old", headers: [["Old", "o"]], body: null },
  });
  assert.equal(out.error, null);
  assert.equal(out.logs[0].text, '{"A":"1","B":"2"} http://new PATCH {"x":1}');
  assert.deepEqual(out.request, { method: "PATCH", url: "http://new", headers: [["A", "1"], ["B", "2"]], body: '{"x":1}' });
});

test("pm.request.url: setter, query upsert and remove, parts of a template URL", async () => {
  const out = await before(`pm.request.url.query.upsert({ key: "a", value: "2" });
pm.request.url.query.remove("b");
pm.request.removeQueryParams(["c"]);
console.log(pm.request.url.getHost(), pm.request.url.getPath(), pm.request.url.getQueryString());
pm.request.url = pm.request.url.toString() + "&d=4";`, {
    request: { method: "GET", url: "{{baseUrl}}/v1/x?a=1&b=2&c=3", headers: [], body: null },
  });
  assert.equal(out.error, null);
  assert.equal(out.logs[0].text, "{{baseUrl}} /v1/x a=2");
  assert.equal(out.request.url, "{{baseUrl}}/v1/x?a=2&d=4");
});

test("pm header lists: each, all, count, toObject, string members", async () => {
  const out = await before(`pm.request.headers.add("X-Str: v");
const seen = [];
pm.request.headers.each((h) => seen.push(h.key));
console.log(seen.join(","), String(pm.request.headers.count()), JSON.stringify(pm.request.headers.toObject()), String(pm.request.headers.has("x-str", "v")));`, {
    request: { method: "GET", url: "http://h", headers: [["A", "1"]], body: null },
  });
  assert.equal(out.logs[0].text, 'A,X-Str 2 {"A":"1","X-Str":"v"} true');
});

test("pm.info.eventName is test after the response; pm.response is undefined before", async () => {
  const a = await runPhase(quick(`console.log(pm.info.eventName, typeof res, typeof responseBody);`));
  assert.equal(a.logs[0].text, "test function string");
  const b = await before(`console.log(String(pm.response), typeof res, typeof responseBody);`);
  assert.equal(b.logs[0].text, "undefined undefined undefined");
});

test("pm.execution.skipRequest is refused after the response", async () => {
  const out = await runPhase(quick(`pm.execution.skipRequest();`));
  assert.match(out.error, /pre-request scripts only/);
});

test("unsupported APIs throw errors that point to SHIM-COVERAGE.md", async () => {
  for (const code of ["pm.environment.clear()", "pm.require('x')", "xml2Json('<a/>')", "bru.cwd()", "req.onFail(() => {})", "insomnia.parentFolders.get('x')", "bru.deleteAllVars()"]) {
    const out = await runPhase(quick(code));
    assert.match(out.error ?? "", /not supported in RequestTailor.*SHIM-COVERAGE\.md/, code);
  }
});

test("setInterval and clearInterval", async () => {
  const out = await before(`let n = 0;
const id = setInterval(() => { n++; if (n === 3) clearInterval(id); }, 5);
await rt.sleep(50);
console.log(String(n));`);
  assert.equal(out.logs[0].text, "3");
});

test("bru res(query) edge cases", async () => {
  const out = await runPhase(
    quick(`console.log(JSON.stringify([res("a.b"), res("list[-1]"), res("missing.x"), res(), res("list.v")]));`, {
      response: { status: 200, headers: [], body: '{"a":{"b":1},"list":[{"v":1},{"v":2}]}', time_ms: 1, url: "" },
    }),
  );
  assert.equal(out.logs[0].text, '[1,{"v":2},null,{"a":{"b":1},"list":[{"v":1},{"v":2}]},[1,2]]');
});
