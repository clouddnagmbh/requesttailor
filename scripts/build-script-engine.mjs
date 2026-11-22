// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Builds the script engine crates/rt-script/engine/rt-script-engine.wasm: QuickJS-ng plus
// the glue in crates/rt-script/engine/src/engine.c, compiled to wasm32-wasip1 (reactor) with
// a pinned wasi-sdk. Pins (versions, URLs, SHA-256) and the output's SHA-256 are in
// crates/rt-script/engine/engine.json.
//
// Usage:
//   node scripts/build-script-engine.mjs           rebuild, update the .wasm and engine.json
//   node scripts/build-script-engine.mjs --check   rebuild into a temporary folder; fail if the
//                                                  bytes differ from the committed .wasm
//
// Downloads go to a cache outside the repo ($RT_CACHE_DIR, default
// ~/.cache/requesttailor/script-engine) and are checked against their pinned SHA-256 before
// use. Hosts: macOS arm64, Linux x86_64 and arm64. No dependencies besides Node, tar, unzip.
// The build is reproducible: same pins and sources, same bytes, on every host (file paths
// are mapped away, the output is stripped).

import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const engineDir = join(repo, "crates/rt-script/engine");
const pinsPath = join(engineDir, "engine.json");
const check = process.argv.includes("--check");

/** Compiler and linker flags. Changing them changes the output (and its recorded SHA-256). */
const TARGET = ["--target=wasm32-wasip1", "-O2"];
const CFLAGS = [...TARGET, "-DNDEBUG", "-fno-ident"];
const LDFLAGS = [
  ...TARGET,
  "-mexec-model=reactor",
  // QuickJS checks its own stack limit (768 KiB, engine.c) inside this shadow stack.
  "-Wl,-z,stack-size=1048576",
  "-Wl,--strip-all",
];

function sha256(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function host() {
  const arch = { arm64: "arm64", x64: "x86_64" }[process.arch];
  const os = { darwin: "macos", linux: "linux" }[process.platform];
  if (!arch || !os || (os === "macos" && arch !== "arm64")) {
    throw new Error(`no pinned wasi-sdk for ${process.platform}/${process.arch}`);
  }
  return `${arch}-${os}`;
}

function run(cmd, args, options = {}) {
  const r = spawnSync(cmd, args, { stdio: ["ignore", "inherit", "inherit"], ...options });
  if (r.error) throw r.error;
  if (r.status !== 0) throw new Error(`${cmd} failed with exit code ${r.status}`);
}

async function download(url, file, want) {
  if (existsSync(file) && sha256(readFileSync(file)) === want) return;
  console.log(`downloading ${url}`);
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  const bytes = Buffer.from(await response.arrayBuffer());
  const got = sha256(bytes);
  if (got !== want) throw new Error(`${url}: SHA-256 ${got}, pinned ${want}`);
  writeFileSync(`${file}.part`, bytes);
  renameSync(`${file}.part`, file);
}

async function main() {
  const pins = JSON.parse(readFileSync(pinsPath, "utf8"));
  const cache = process.env.RT_CACHE_DIR || join(homedir(), ".cache/requesttailor/script-engine");
  mkdirSync(join(cache, "dl"), { recursive: true });

  // wasi-sdk: clang, lld and wasi-libc for wasm32-wasip1.
  const h = host();
  const sdkSha = pins.wasi_sdk.sha256[h];
  if (!sdkSha) throw new Error(`no pinned wasi-sdk SHA-256 for ${h}`);
  const sdkName = `wasi-sdk-${pins.wasi_sdk.version}-${h}`;
  const sdkTar = join(cache, "dl", `${sdkName}.tar.gz`);
  await download(pins.wasi_sdk.url.replace("{host}", h), sdkTar, sdkSha);
  const sdk = join(cache, `${sdkName}-${sdkSha.slice(0, 12)}`);
  if (!existsSync(join(sdk, "bin/clang"))) {
    rmSync(sdk, { recursive: true, force: true });
    mkdirSync(sdk, { recursive: true });
    run("tar", ["-xzf", sdkTar, "-C", sdk, "--strip-components=1"]);
  }

  // QuickJS-ng amalgam (quickjs-amalgam.c, quickjs.h).
  const qjsZip = join(cache, "dl", `quickjs-amalgam-${pins.quickjs.version}.zip`);
  await download(pins.quickjs.url, qjsZip, pins.quickjs.sha256);
  const qjs = join(cache, `quickjs-${pins.quickjs.version}-${pins.quickjs.sha256.slice(0, 12)}`);
  if (!existsSync(join(qjs, "quickjs-amalgam.c"))) {
    rmSync(qjs, { recursive: true, force: true });
    mkdirSync(qjs, { recursive: true });
    run("unzip", ["-q", "-o", qjsZip, "-d", qjs]);
  }

  const work = mkdtempSync(join(tmpdir(), "rt-script-engine-"));
  try {
    const clang = join(sdk, "bin/clang");
    const common = [
      ...CFLAGS,
      `-I${qjs}`,
      // Reproducible: no host paths in the output.
      `-ffile-prefix-map=${qjs}=quickjs`,
      `-ffile-prefix-map=${repo}=.`,
      `-ffile-prefix-map=${work}=build`,
    ];
    const opts = { cwd: work };
    run(clang, [...common, "-w", "-c", join(qjs, "quickjs-amalgam.c"), "-o", "quickjs.o"], opts);
    run(clang, [...common, "-Wall", "-Wextra", "-Wno-unused-parameter", "-c", join(engineDir, "src/engine.c"), "-o", "engine.o"], opts);
    run(clang, [...LDFLAGS, "engine.o", "quickjs.o", "-o", "rt-script-engine.wasm"], opts);
    const bytes = readFileSync(join(work, "rt-script-engine.wasm"));
    const out = join(engineDir, pins.output.file);
    const digest = sha256(bytes);
    if (check) {
      const committed = existsSync(out) ? readFileSync(out) : Buffer.alloc(0);
      if (!committed.equals(bytes)) {
        console.error(`script engine differs: rebuilt ${digest} (${bytes.length} bytes), committed ${sha256(committed)} (${committed.length} bytes)`);
        process.exit(1);
      }
      if (pins.output.sha256 !== digest || pins.output.size !== bytes.length) {
        console.error(`engine.json records ${pins.output.sha256} (${pins.output.size} bytes), the engine is ${digest} (${bytes.length} bytes)`);
        process.exit(1);
      }
      console.log(`script engine ok: ${digest} (${bytes.length} bytes)`);
      return;
    }
    writeFileSync(out, bytes);
    pins.output.size = bytes.length;
    pins.output.sha256 = digest;
    writeFileSync(pinsPath, `${JSON.stringify(pins, null, 2)}\n`);
    console.log(`wrote ${out}: ${digest} (${bytes.length} bytes)`);
  } finally {
    rmSync(work, { recursive: true, force: true });
  }
}

main().catch((e) => {
  console.error(`build-script-engine: ${e.message}`);
  process.exit(1);
});
