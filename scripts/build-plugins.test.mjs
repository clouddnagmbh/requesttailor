// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { checkPluginModule, linkArgs, parseArgs, pluginPackages } from "./build-plugins.mjs";

// ---- a minimal WebAssembly encoder, enough for interface tests ----

const enc = new TextEncoder();
const leb = (n) => {
  const out = [];
  do {
    let byte = n & 0x7f;
    n >>>= 7;
    if (n) byte |= 0x80;
    out.push(byte);
  } while (n);
  return out;
};
const name = (s) => [...leb(enc.encode(s).length), ...enc.encode(s)];
const section = (id, payload) => [id, ...leb(payload.length), ...payload];

/** A module with `() -> ()` imports and exports, optional memory and custom sections. */
function module({ imports = [], exports = [], memory = true, custom = [] }) {
  const bytes = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
  bytes.push(...section(1, [1, 0x60, 0, 0]));
  if (imports.length) {
    bytes.push(...section(2, [...leb(imports.length), ...imports.flatMap(([m, n]) => [...name(m), ...name(n), 0, 0])]));
  }
  bytes.push(...section(3, [...leb(exports.length), ...exports.map(() => 0)]));
  if (memory) bytes.push(...section(5, [1, 0, 1]));
  const exp = exports.map((e, i) => [...name(e), 0, ...leb(imports.length + i)]);
  if (memory) exp.push([...name("memory"), 2, 0]);
  bytes.push(...section(7, [...leb(exp.length), ...exp.flat()]));
  bytes.push(...section(10, [...leb(exports.length), ...exports.flatMap(() => [2, 0, 0x0b])]));
  for (const [n, payload] of custom) bytes.push(...section(0, [...name(n), ...enc.encode(payload)]));
  return new Uint8Array(bytes);
}

const ABI = ["rt_alloc", "rt_dealloc", "rt_call"];
const MANIFEST = JSON.stringify({ id: "demo", name: "Demo", version: "0.1.0", api: 1 });

test("a module with the ABI, rt.host and the manifest passes", () => {
  const { manifest, problems } = checkPluginModule(
    module({ imports: [["rt", "host"], ["wasi_snapshot_preview1", "fd_write"]], exports: ABI, custom: [["rt-plugin", MANIFEST]] }),
  );
  assert.deepEqual(problems, []);
  assert.equal(manifest.id, "demo");
});

test("imports outside rt.host and the WASI subset are rejected", () => {
  const { problems } = checkPluginModule(
    module({
      imports: [["__wbindgen_placeholder__", "__wbindgen_describe"], ["wasi_snapshot_preview1", "fd_read"]],
      exports: ABI,
      custom: [["rt-plugin", MANIFEST]],
    }),
  );
  assert.deepEqual(problems, [
    "import not allowed: __wbindgen_placeholder__.__wbindgen_describe (function)",
    "import not allowed: wasi_snapshot_preview1.fd_read (function)",
  ]);
});

test("missing exports, manifest problems and non-wasm input are reported", () => {
  assert.deepEqual(checkPluginModule(module({ exports: ["rt_call"], memory: false })).problems, [
    "missing export: memory",
    "missing export: rt_alloc",
    "missing export: rt_dealloc",
    'expected one "rt-plugin" custom section, found 0',
  ]);
  assert.deepEqual(checkPluginModule(module({ exports: ABI, custom: [["rt-plugin", "{nope"]] })).problems.length, 1);
  const badId = JSON.stringify({ id: "Demo", name: "D", version: "1", api: 1 });
  assert.deepEqual(checkPluginModule(module({ exports: ABI, custom: [["rt-plugin", badId]] })).problems, [
    'manifest id "Demo" is invalid',
  ]);
  const twice = module({ exports: ABI, custom: [["rt-plugin", MANIFEST], ["rt-plugin", MANIFEST]] });
  assert.equal(checkPluginModule(twice).problems.length, 1);
  assert.match(checkPluginModule(new Uint8Array([1, 2, 3])).problems[0], /^not WebAssembly/);
});

test("link arguments export exactly the ABI", () => {
  const args = linkArgs("lib.a", "out.wasm");
  assert.deepEqual(
    args.filter((a) => a.startsWith("--export")),
    ["--export=rt_alloc", "--export=rt_dealloc", "--export=rt_call"],
  );
  assert.deepEqual(args.slice(-3), ["-o", "out.wasm", "lib.a"]);
  assert.ok(args.includes("--gc-sections") && args.includes("--no-entry"));
});

test("plugin crates are the packages directly in plugins/ with a plugin.json", () => {
  const root = mkdtempSync(join(tmpdir(), "build-plugins-"));
  try {
    for (const dir of ["plugins/a", "plugins/b", "plugins/a/nested", "crates/c"]) {
      mkdirSync(join(root, dir), { recursive: true });
    }
    writeFileSync(join(root, "plugins/a/plugin.json"), "{}");
    writeFileSync(join(root, "plugins/a/nested/plugin.json"), "{}");
    writeFileSync(join(root, "crates/c/plugin.json"), "{}");
    const pkg = (name, dir) => ({
      name,
      manifest_path: join(root, dir, "Cargo.toml"),
      targets: [{ name, kind: ["cdylib", "rlib"] }],
    });
    const metadata = {
      packages: [pkg("rt-plugin-a", "plugins/a"), pkg("b", "plugins/b"), pkg("n", "plugins/a/nested"), pkg("c", "crates/c")],
    };
    assert.deepEqual(pluginPackages(metadata, root), [
      { name: "rt-plugin-a", dir: join(root, "plugins/a"), folder: "a", libName: "rt_plugin_a" },
    ]);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test("arguments", () => {
  assert.deepEqual(parseArgs(["example", "--out", "dist"]), { outDir: "dist", names: ["example"], optional: false });
  assert.equal(parseArgs(["--optional", "odata"]).optional, true);
  assert.throws(() => parseArgs(["--out"]));
  assert.throws(() => parseArgs(["--release"]));
});
