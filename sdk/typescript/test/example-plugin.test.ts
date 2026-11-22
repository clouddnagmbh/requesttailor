// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The built example plugin (`target/plugins/example.wasm`, from `scripts/build-plugins.mjs`)
// on Node's WebAssembly with the test host: proves the binary interface of API.md (exports,
// `rt.host`, buffer ownership, manifest section) end to end, outside of Rust.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import type {
  Call,
  DetectReply,
  ImportReply,
  NotesReply,
  PluginRequest,
  PrepareReply,
  RequestSpec,
  ViewReply,
} from "../src/api.ts";
import { TestPlugin } from "../src/testing.ts";

const wasmPath = new URL("../../../target/plugins/example.wasm", import.meta.url);
const wasm = readFileSync(wasmPath);

const request: PluginRequest = {
  method: "GET",
  url: "https://api.example.com/users?key=***",
  headers: [["Accept", "application/json"]],
  body: null,
};
const context = { request_path: "users/get.yml", environment: "Local", variables: [] };

const spec: RequestSpec = {
  name: "Get users",
  seq: 1,
  method: "GET",
  url: "{{base}}/users",
  headers: [],
  params: [],
  body: { type: "none" },
  auth: { type: "inherit" },
  vars: [],
  settings: { timeout_ms: null, follow_redirects: null, max_redirects: null, encode_url: null },
  scripts: [],
  assertions: [],
  actions: [],
};

function ok<T>(reply: { ok?: T | null; error?: string | null }): T {
  assert.equal(reply.error ?? null, null, `error reply: ${reply.error}`);
  return reply.ok as T;
}

test("the module has the interface of API.md and the manifest", async () => {
  const plugin = await TestPlugin.load(wasm);
  assert.equal(plugin.manifest.id, "example");
  assert.equal(plugin.manifest.api, 1);
  assert.deepEqual(plugin.manifest.permissions, { network: [], storage: false, variables: false });
  const imports = WebAssembly.Module.imports(new WebAssembly.Module(wasm));
  assert.deepEqual(
    imports.map((i) => `${i.module}.${i.name}`),
    ["rt.host"],
  );
});

test("prepare sets X-Request-ID through rt_call, using the now host call", async () => {
  const plugin = await TestPlugin.load(wasm, { now: () => 1_790_000_000_123 });
  const reply = ok(plugin.call<PrepareReply>({ hook: "prepare", request, context }));
  const [[name, id]] = reply.patch?.set_headers ?? [];
  assert.equal(name, "X-Request-ID");
  assert.match(id, /^rt-01a0c[0-9a-f]{7}-[0-9a-f]{8}$/);
  assert.deepEqual(reply.notes, [{ level: "info", text: `X-Request-ID: ${id}` }]);
  assert.deepEqual(plugin.hostCalls, [{ op: "now" }]);
});

test("response, tab, action, detect and import answer like the native tests", async () => {
  const plugin = await TestPlugin.load(wasm);
  const response = {
    status: 200,
    headers: [["Content-Type", "application/json"]] as [string, string][],
    url: "https://api.example.com/users",
    body: { content_type: "application/json", text: '{"a": 1}' },
    truncated: false,
  };
  const notes = ok(plugin.call<NotesReply>({ hook: "response", request, response, context }));
  assert.deepEqual(notes.notes, [{ level: "info", text: "200 · 8 B · application/json" }]);

  const target = { request_path: "users/get.yml", response_id: 1 };
  const view = ok(
    plugin.call<ViewReply>({ hook: "render-tab", tab: "summary", target, spec, response, state: null }),
  );
  assert.deepEqual(view.state, { stats: false });
  assert.deepEqual(view.view.blocks[0], { type: "heading", text: "GET {{base}}/users" });

  const action: Call = {
    hook: "action",
    tab: "summary",
    action: "body-stats",
    target,
    spec,
    response,
    state: view.state,
    input: [],
  };
  const stats = ok(plugin.call<ViewReply>(action));
  assert.deepEqual(stats.state, { stats: true });
  assert.ok(stats.view.blocks.some((b) => b.type === "key-value" && b.rows.some(([k]) => k === "JSON")));

  const file = { name: "list.urls", base64: Buffer.from("GET https://a.example/x\n").toString("base64") };
  const detect = ok(plugin.call<DetectReply>({ hook: "detect", importer: "url-list", files: [file] }));
  assert.equal(detect.matches, true);
  const imported = ok(plugin.call<ImportReply>({ hook: "import", importer: "url-list", files: [file] }));
  assert.equal(imported.collection_name, "list");
  assert.deepEqual(
    imported.files.map(([path]) => path),
    ["opencollection.yml", "1-get-x.yml"],
  );
});

test("errors come back as error replies, and the plugin keeps working", async () => {
  const plugin = await TestPlugin.load(wasm);
  assert.match(JSON.parse(plugin.callRaw("not json")).error, /^invalid call/);
  assert.deepEqual(JSON.parse(plugin.callRaw('{"hook": "teleport"}')), { error: "unsupported hook: teleport" });
  const reply = plugin.call({ hook: "import", importer: "nope", files: [] });
  assert.deepEqual(reply, { error: "unknown importer: nope" });
  ok(plugin.call<PrepareReply>({ hook: "prepare", request, context }));
});

test("buffers are freed: many calls do not grow memory", async () => {
  const plugin = await TestPlugin.load(wasm);
  const big: PluginRequest = { ...request, url: `https://api.example.com/${"x".repeat(64 * 1024)}` };
  for (let i = 0; i < 50; i++) ok(plugin.call<PrepareReply>({ hook: "prepare", request: big, context }));
  const after50 = plugin.memoryBytes();
  for (let i = 0; i < 500; i++) ok(plugin.call<PrepareReply>({ hook: "prepare", request: big, context }));
  assert.equal(plugin.memoryBytes(), after50);
});
