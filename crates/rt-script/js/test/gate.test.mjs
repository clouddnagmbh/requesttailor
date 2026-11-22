// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The wave 7 gate collection (testdata/import/postman/scripts-gate.postman_collection.json)
// on the fake engine: a small runner sends its three requests in order to an echo that
// answers like rt-testserver's `/echo`, keeps the variables the scripts set, and checks that
// every test passes. The real run (`rt run` against rt-testserver) belongs to the runner.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { CASES_DIR, runPhase } from "./fake-rt.mjs";

const GATE = join(CASES_DIR, "../../../../testdata/import/postman/scripts-gate.postman_collection.json");
const BASE = "http://127.0.0.1:47811";

const scriptsOf = (events, listen, origin) =>
  (events ?? []).filter((e) => e.listen === listen).map((e) => ({ origin, code: e.script.exec.join("\n") }));

/** Requests in run order with the scripts of their collection and folders. */
function plan(collection) {
  const out = [];
  const walk = (items, parents) => {
    for (const item of items) {
      if (item.item) walk(item.item, [...parents, item]);
      else out.push({ item, parents });
    }
  };
  walk(collection.item, []);
  return out;
}

/** What rt-testserver's `/echo` answers. */
function echo(method, url, headers, body) {
  const u = new URL(url);
  const json = {
    method,
    path: u.pathname,
    query: u.search.slice(1),
    headers: headers.map(([n, v]) => [n.toLowerCase(), v]),
    body,
  };
  return { status: 200, headers: [["content-type", "application/json"]], body: JSON.stringify(json), time_ms: 3, url };
}

test("the gate collection runs green on the shims", async () => {
  const collection = JSON.parse(readFileSync(GATE, "utf8"));
  const layers = { collection: new Map(collection.variable.map((v) => [v.key, v.value])), runtime: new Map([["baseUrl", BASE]]) };
  const variables = () =>
    ["collection", "runtime"].flatMap((layer) => [...layers[layer]].map(([name, value]) => ({ name, value, source: { layer }, secret: false })));
  const resolve = (s) =>
    s.replace(/\{\{([^{}]+)\}\}/g, (m, name) => layers.runtime.get(name) ?? layers.collection.get(name) ?? m);
  const apply = (changes) => {
    for (const c of changes) {
      const layer = c.scope === "runtime" ? layers.runtime : layers.collection;
      if (c.value === null) layer.delete(c.name);
      else layer.set(c.name, c.value);
    }
  };
  const results = [];
  for (const { item, parents } of plan(collection)) {
    const r = item.request;
    const info = { request_name: item.name, request_path: item.name, collection_name: collection.info.name };
    const request = {
      method: r.method,
      url: r.url.raw,
      headers: r.header.map((h) => [h.key, h.value]),
      body: r.body?.raw ?? null,
    };
    const before = await runPhase({
      phase: "before-request",
      scripts: [
        ...scriptsOf(collection.event, "prerequest", "collection"),
        ...parents.flatMap((p) => scriptsOf(p.event, "prerequest", `folder ${p.name}`)),
        ...scriptsOf(item.event, "prerequest", "request"),
      ],
      request,
      variables: variables(),
      info,
      send: [{ url: `${BASE}/status/201`, response: { status: 201, headers: [], body: '{"status":201}', time_ms: 1, url: `${BASE}/status/201` } }],
    });
    assert.equal(before.error, null, `${item.name}: ${before.error}`);
    apply(before.var_changes);
    const sent = before.request ?? request;
    const url = resolve(sent.url);
    const headers = sent.headers.map(([n, v]) => [n, resolve(v)]);
    const body = sent.body === null ? null : resolve(sent.body);
    const after = await runPhase({
      phase: "tests",
      scripts: [...scriptsOf(collection.event, "test", "collection"), ...scriptsOf(item.event, "test", "request")],
      request: { method: sent.method, url, headers, body },
      response: echo(sent.method, url, headers, body),
      variables: variables(),
      info,
    });
    assert.equal(after.error, null, `${item.name}: ${after.error}`);
    apply(after.var_changes);
    results.push(...after.tests.map((t) => ({ request: item.name, ...t })));
  }
  const failed = results.filter((t) => !t.passed);
  assert.deepEqual(failed, []);
  assert.equal(results.length, 14);
  assert.equal(layers.collection.get("createdOrderId"), "ord-42");
  assert.equal(layers.collection.get("requestCount"), "3");
});
