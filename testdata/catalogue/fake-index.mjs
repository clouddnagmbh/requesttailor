#!/usr/bin/env node
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// A fake plugin index host for the plugin catalogue (wave 18, strand I). Tests run against this
// and never against a real host. Node only, no dependencies. The modules are the synthetic
// fixtures of crates/rt-plugin/tests/fixtures (`demo` 1.0.0 with storage, `demo` 2.0.0 that also
// asks for network access).
//
//   node fake-index.mjs [--index-port 48629] [--git-port 48630]
//
// Index host (default 48629):
// - `GET /cors/plugins.json`, `/cors/<module>.wasm`: with `Access-Control-Allow-Origin: *`.
// - `GET /nocors/plugins.json`, `/nocors/<module>.wasm`: the same without CORS headers (a
//   browser may not read them; `rt` and the desktop app can).
// - The index lists `demo` at the version set by `POST /__release?version=1.0.0|2.0.0` (default
//   1.0.0); `POST /__release?version=1.0.0&tampered=1` publishes other bytes under the same
//   version with the matching checksum (a changed release); `POST /__break?what=checksum|none`
//   makes the index list a wrong checksum.
// - `GET /__calls` the requests seen (method and path, in order), `POST /__reset` clears the log
//   and the release.
// Git host (default 48630): the Gitea raw route `GET /org/plugins/raw/branch/main/plugins.json`
// (no CORS headers needed: the modules come from the index host's absolute addresses).
// On start it prints one line `READY {json}`; it exits when stdin closes unless --no-stdin-exit.

import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const fixtures = join(here, "../../crates/rt-plugin/tests/fixtures");
const args = process.argv.slice(2);
const opt = (name, fallback) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : fallback;
};
const INDEX_PORT = Number(opt("index-port", 48629));
const GIT_PORT = Number(opt("git-port", 48630));

const sha = (b) => createHash("sha256").update(b).digest("hex");
const modules = {
  "1.0.0": readFileSync(join(fixtures, "demo.wasm")),
  "2.0.0": readFileSync(join(fixtures, "demo-v2.wasm")),
};
/** The same plugin and version with different bytes: a valid trailing custom section. */
const tampered = (b) => Buffer.concat([b, Buffer.from([0x00, 0x02, 0x01, 0x78])]);

const state = { calls: [], version: "1.0.0", tampered: false, breakChecksum: false };

function bytesOf(version) {
  const b = modules[version];
  return state.tampered ? tampered(b) : b;
}

function indexJson(origin) {
  const version = state.version;
  const wasm = bytesOf(version);
  const claimed = version === "1.0.0" ? { storage: true } : { network: ["{request}"], storage: true };
  return JSON.stringify({
    version: 1,
    name: "Fake index",
    plugins: [
      {
        id: "demo",
        name: "Demo plugin",
        version,
        api: 1,
        description: "Shows what a plugin can do: a header, notes, two tabs and an importer.",
        license: "Apache-2.0",
        wasm: {
          url: origin ? `${origin}/cors/demo-${version}.wasm` : `demo-${version}.wasm`,
          sha256: state.breakChecksum ? "0".repeat(64) : sha(wasm),
          size: wasm.length,
        },
        permissions: claimed,
      },
    ],
  });
}

function send(res, status, body, headers = {}) {
  res.writeHead(status, { "Content-Length": Buffer.byteLength(body), ...headers });
  res.end(body);
}

const indexServer = createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  if (url.pathname === "/__calls") return send(res, 200, JSON.stringify(state.calls), { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" });
  state.calls.push(`${req.method} ${url.pathname}`);
  if (req.method === "OPTIONS") {
    return send(res, 204, "", { "Access-Control-Allow-Origin": "*", "Access-Control-Allow-Headers": "*", "Access-Control-Allow-Methods": "GET, POST" });
  }
  if (url.pathname === "/__reset") {
    state.calls = [];
    Object.assign(state, { version: "1.0.0", tampered: false, breakChecksum: false });
    return send(res, 200, "{}", { "Access-Control-Allow-Origin": "*" });
  }
  if (url.pathname === "/__release") {
    const v = url.searchParams.get("version");
    if (!modules[v]) return send(res, 400, "unknown version");
    state.version = v;
    state.tampered = url.searchParams.get("tampered") === "1";
    return send(res, 200, "{}", { "Access-Control-Allow-Origin": "*" });
  }
  if (url.pathname === "/__break") {
    state.breakChecksum = url.searchParams.get("what") === "checksum";
    return send(res, 200, "{}", { "Access-Control-Allow-Origin": "*" });
  }
  const m = /^\/(cors|nocors)\/(.+)$/.exec(url.pathname);
  if (!m) return send(res, 404, "not found");
  const cors = m[1] === "cors" ? { "Access-Control-Allow-Origin": "*" } : {};
  if (m[2] === "plugins.json") {
    return send(res, 200, indexJson(null), { "Content-Type": "application/json", ...cors });
  }
  const w = /^demo-(\d+\.\d+\.\d+)\.wasm$/.exec(m[2]);
  if (w && modules[w[1]]) {
    // The listed version may be the tampered one; the module of another version is plain.
    const body = w[1] === state.version ? bytesOf(w[1]) : modules[w[1]];
    return send(res, 200, body, { "Content-Type": "application/wasm", ...cors });
  }
  return send(res, 404, "not found", cors);
});

const gitServer = createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  state.calls.push(`GIT ${req.method} ${url.pathname}`);
  if (url.pathname === "/org/plugins/raw/branch/main/plugins.json") {
    return send(res, 200, indexJson(`http://127.0.0.1:${INDEX_PORT}`), { "Content-Type": "application/json", "Access-Control-Allow-Origin": "*" });
  }
  return send(res, 404, "not found", { "Access-Control-Allow-Origin": "*" });
});

await new Promise((r) => indexServer.listen(INDEX_PORT, "127.0.0.1", r));
await new Promise((r) => gitServer.listen(GIT_PORT, "127.0.0.1", r));
console.log(`READY ${JSON.stringify({ indexPort: indexServer.address().port, gitPort: gitServer.address().port })}`);

if (!args.includes("--no-stdin-exit")) {
  process.stdin.resume();
  process.stdin.on("end", () => process.exit(0));
}
process.on("SIGTERM", () => process.exit(0));
