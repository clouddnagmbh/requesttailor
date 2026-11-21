// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { lockedVersion, upToDate } from "./build-wasm.mjs";

test("reads a crate version from Cargo.lock", () => {
  const lock = [
    "[[package]]",
    'name = "wasm-bindgen-futures"',
    'version = "0.4.1"',
    "",
    "[[package]]",
    'name = "wasm-bindgen"',
    'version = "0.2.130"',
    "",
  ].join("\n");
  assert.equal(lockedVersion(lock, "wasm-bindgen"), "0.2.130");
  assert.equal(lockedVersion(lock, "serde"), null);
});

test("output is up to date only when newer than every input", () => {
  const dir = mkdtempSync(join(tmpdir(), "build-wasm-"));
  try {
    const src = join(dir, "src");
    mkdirSync(join(src, "nested"), { recursive: true });
    const input = join(src, "nested", "lib.rs");
    const output = join(dir, "out.wasm");
    writeFileSync(input, "");
    assert.equal(upToDate(output, [src]), false, "missing output");
    writeFileSync(output, "");
    utimesSync(input, 1000, 1000);
    utimesSync(output, 2000, 2000);
    assert.equal(upToDate(output, [src, join(dir, "missing")]), true);
    utimesSync(input, 3000, 3000);
    assert.equal(upToDate(output, [src]), false, "input changed");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
