// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Builds the core for the web app: rt-wasm for wasm32 (release), then wasm-bindgen
// (`--target web`) into apps/web/public/wasm/ (gitignored), served at `/wasm/rt_wasm.js`.
//
// Usage: node scripts/build-wasm.mjs [--optional] [--force]
//   --optional  missing toolchain or a failed build only warns (exit 0): `npm run dev -w
//               apps/web` uses this, so `?backend=mock` keeps working without Rust.
//   --force     build even if the output is newer than every input.
// Skips the build when the output is newer than all Rust sources it depends on.
//
// Needs: the pinned toolchain with the wasm32-unknown-unknown target, and wasm-bindgen-cli of
// the version in Cargo.lock (`cargo install wasm-bindgen-cli --version <v> --locked`).

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
export const OUT_DIR = join(ROOT, "apps", "web", "public", "wasm");
const CRATES = ["rt-core", "rt-format", "rt-app", "rt-wasm"];
const TARGET = "wasm32-unknown-unknown";

/** Version of `name` in a Cargo.lock text, or null. */
export function lockedVersion(lockText, name) {
  const m = lockText.match(new RegExp(`\\[\\[package\\]\\]\\nname = "${name}"\\nversion = "([^"]+)"`));
  return m ? m[1] : null;
}

/** Newest modification time (ms) of the files below `path` (a file or a folder). */
export function newestMtime(path) {
  if (!existsSync(path)) return 0;
  const st = statSync(path);
  if (!st.isDirectory()) return st.mtimeMs;
  let newest = 0;
  for (const entry of readdirSync(path)) newest = Math.max(newest, newestMtime(join(path, entry)));
  return newest;
}

/** True when `output` exists and is newer than every input. */
export function upToDate(output, inputs) {
  if (!existsSync(output)) return false;
  const built = statSync(output).mtimeMs;
  return inputs.every((input) => newestMtime(input) <= built);
}

function inputs() {
  const list = [join(ROOT, "Cargo.toml"), join(ROOT, "Cargo.lock"), join(ROOT, "rust-toolchain.toml")];
  for (const c of CRATES) {
    for (const part of ["src", "Cargo.toml", "build.rs"]) list.push(join(ROOT, "crates", c, part));
  }
  return list;
}

// Never let rustup download a toolchain behind the user's back from here.
const ENV = { ...process.env, RUSTUP_AUTO_INSTALL: "0" };

function run(cmd, args) {
  return execFileSync(cmd, args, { cwd: ROOT, env: ENV, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** Returns a problem description, or null when everything needed is installed. */
function checkToolchain(wantBindgen) {
  try {
    run("cargo", ["--version"]);
  } catch {
    return "cargo (the pinned Rust toolchain from rust-toolchain.toml) is not installed";
  }
  try {
    const libdir = run("rustc", ["--print", "target-libdir", "--target", TARGET]);
    if (!existsSync(libdir)) throw new Error();
  } catch {
    return `the ${TARGET} target is missing: rustup target add ${TARGET}`;
  }
  let have = null;
  try {
    have = run("wasm-bindgen", ["--version"]).split(/\s+/)[1];
  } catch {
    // reported below
  }
  if (have !== wantBindgen) {
    return `wasm-bindgen ${wantBindgen} is needed (found: ${have ?? "none"}): cargo install wasm-bindgen-cli --version ${wantBindgen} --locked`;
  }
  return null;
}

function main() {
  const optional = process.argv.includes("--optional");
  const force = process.argv.includes("--force");
  const output = join(OUT_DIR, "rt_wasm_bg.wasm");
  const fail = (msg) => {
    if (optional) {
      console.warn(`build-wasm: ${msg}\nbuild-wasm: the real web backend is unavailable; ?backend=mock still works.`);
      process.exit(0);
    }
    console.error(`build-wasm: ${msg}`);
    process.exit(1);
  };

  if (!force && upToDate(output, inputs())) {
    console.log("build-wasm: apps/web/public/wasm is up to date");
    return;
  }
  const wantBindgen = lockedVersion(readFileSync(join(ROOT, "Cargo.lock"), "utf8"), "wasm-bindgen");
  const problem = checkToolchain(wantBindgen);
  if (problem) fail(problem);

  console.log("build-wasm: cargo build -p rt-wasm --target wasm32-unknown-unknown --release");
  const build = spawnSync(
    "cargo",
    ["build", "-p", "rt-wasm", "--target", TARGET, "--release", "--locked"],
    { cwd: ROOT, env: ENV, stdio: "inherit" },
  );
  if (build.status !== 0) fail("cargo build of rt-wasm failed");

  const targetDir = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps"])).target_directory;
  const wasm = join(targetDir, TARGET, "release", "rt_wasm.wasm");
  mkdirSync(OUT_DIR, { recursive: true });
  const bindgen = spawnSync(
    "wasm-bindgen",
    ["--target", "web", "--no-typescript", "--out-dir", OUT_DIR, wasm],
    { cwd: ROOT, env: ENV, stdio: "inherit" },
  );
  if (bindgen.status !== 0) fail("wasm-bindgen failed");
  const size = statSync(output).size;
  console.log(`build-wasm: wrote apps/web/public/wasm (rt_wasm_bg.wasm ${(size / 1024).toFixed(0)} KiB)`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
