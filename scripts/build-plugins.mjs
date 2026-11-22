// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Builds the Rust plugins in plugins/ (every crate there with a plugin.json) into single
// WebAssembly modules, target/plugins/<id>.wasm, and checks each against the binary
// interface of crates/rt-plugin/API.md.
//
// Usage: node scripts/build-plugins.mjs [--out DIR] [NAME ...]
//   NAME   a plugin folder name (e.g. `example`); default: all plugins.
//   --out  output folder instead of target/plugins.
//
// Why not `cargo build` with crate type cdylib: a cdylib exports every `#[no_mangle]` symbol
// of every dependency. The API types depend on rt-core, whose wasm32 build carries the
// browser transport (wasm-bindgen), so a cdylib also exports wasm-bindgen's helpers and
// imports `__wbindgen_*` functions, which no plugin host links. Instead the crate is built
// as a staticlib (profile `plugin`: release, size-optimized, LTO) and linked with the
// toolchain's own rust-lld, exporting exactly `rt_alloc`, `rt_dealloc` and `rt_call`; the
// linker drops everything else, and with it the unused imports.
//
// Needs: the pinned toolchain with the wasm32-unknown-unknown target. No other tools.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join, relative, sep } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
export const DEFAULT_OUT = join(ROOT, "target", "plugins");
const TARGET = "wasm32-unknown-unknown";
const PROFILE = "plugin";

/** Cargo `--config` values for the `plugin` profile (kept out of the workspace Cargo.toml). */
export const PROFILE_CONFIG = [
  `profile.${PROFILE}.inherits="release"`,
  `profile.${PROFILE}.opt-level="s"`,
  `profile.${PROFILE}.lto=true`,
  `profile.${PROFILE}.codegen-units=1`,
];

/** The exports of API.md. */
export const ABI_EXPORTS = ["rt_alloc", "rt_dealloc", "rt_call"];

/** The `wasi_snapshot_preview1` functions API.md allows (for plugins compiled from JS/TS). */
export const WASI_SUBSET = [
  "fd_write",
  "clock_time_get",
  "random_get",
  "environ_get",
  "environ_sizes_get",
  "args_get",
  "args_sizes_get",
  "proc_exit",
];

export const MANIFEST_SECTION = "rt-plugin";

/** wasm-ld arguments: exactly the ABI exports, rustc's defaults for wasm32 otherwise. */
export function linkArgs(staticlib, output) {
  return [
    "-flavor",
    "wasm",
    "--no-entry",
    "--gc-sections",
    "-O3",
    "--strip-debug",
    "-z",
    "stack-size=1048576",
    "--stack-first",
    ...ABI_EXPORTS.map((name) => `--export=${name}`),
    "-o",
    output,
    staticlib,
  ];
}

/**
 * Checks a module against API.md. Returns `{ manifest, problems }`; `manifest` is null when
 * the custom section is missing or not JSON.
 */
export function checkPluginModule(bytes) {
  const problems = [];
  let module;
  try {
    module = new WebAssembly.Module(bytes);
  } catch (e) {
    return { manifest: null, problems: [`not WebAssembly: ${e.message}`] };
  }
  for (const i of WebAssembly.Module.imports(module)) {
    const allowed =
      i.kind === "function" &&
      ((i.module === "rt" && i.name === "host") ||
        (i.module === "wasi_snapshot_preview1" && WASI_SUBSET.includes(i.name)));
    if (!allowed) problems.push(`import not allowed: ${i.module}.${i.name} (${i.kind})`);
  }
  const exports = new Map(WebAssembly.Module.exports(module).map((e) => [e.name, e.kind]));
  if (exports.get("memory") !== "memory") problems.push("missing export: memory");
  for (const name of ABI_EXPORTS) {
    if (exports.get(name) !== "function") problems.push(`missing export: ${name}`);
  }
  const sections = WebAssembly.Module.customSections(module, MANIFEST_SECTION);
  let manifest = null;
  if (sections.length !== 1) {
    problems.push(`expected one "${MANIFEST_SECTION}" custom section, found ${sections.length}`);
  } else {
    try {
      manifest = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(sections[0]));
    } catch (e) {
      problems.push(`manifest is not UTF-8 JSON: ${e.message}`);
    }
  }
  if (manifest && !/^[a-z0-9][a-z0-9-]{0,63}$/.test(String(manifest.id))) {
    problems.push(`manifest id ${JSON.stringify(manifest.id)} is invalid`);
  }
  return { manifest, problems };
}

/** The plugin crates of `cargo metadata` output: packages below plugins/ with a plugin.json. */
export function pluginPackages(metadata, root = ROOT) {
  return metadata.packages
    .filter((p) => {
      const dir = dirname(p.manifest_path);
      const rel = relative(join(root, "plugins"), dir);
      return rel && !rel.startsWith("..") && !rel.includes(sep) && existsSync(join(dir, "plugin.json"));
    })
    .map((p) => {
      const lib = p.targets.find((t) => t.kind.includes("lib") || t.kind.includes("cdylib"));
      return {
        name: p.name,
        dir: dirname(p.manifest_path),
        folder: relative(join(root, "plugins"), dirname(p.manifest_path)),
        libName: (lib?.name ?? p.name).replaceAll("-", "_"),
      };
    })
    .sort((a, b) => a.folder.localeCompare(b.folder));
}

export function parseArgs(argv) {
  const out = { outDir: DEFAULT_OUT, names: [] };
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === "--out") {
      if (!argv[i + 1]) throw new Error("--out needs a folder");
      out.outDir = argv[++i];
    } else if (argv[i].startsWith("-")) {
      throw new Error(`unknown option ${argv[i]}`);
    } else {
      out.names.push(argv[i]);
    }
  }
  return out;
}

// Never let rustup download a toolchain behind the user's back from here.
const ENV = { ...process.env, RUSTUP_AUTO_INSTALL: "0" };

function run(cmd, args) {
  return execFileSync(cmd, args, { cwd: ROOT, env: ENV, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] }).trim();
}

/** rust-lld of the pinned toolchain. */
function rustLld() {
  const sysroot = run("rustc", ["--print", "sysroot"]);
  const host = run("rustc", ["-vV"]).match(/^host: (.+)$/m)?.[1];
  const exe = process.platform === "win32" ? "rust-lld.exe" : "rust-lld";
  const path = join(sysroot, "lib", "rustlib", host ?? "", "bin", exe);
  if (!existsSync(path)) throw new Error(`rust-lld not found at ${path}`);
  return path;
}

function main() {
  const fail = (msg) => {
    console.error(`build-plugins: ${msg}`);
    process.exit(1);
  };
  let args;
  try {
    args = parseArgs(process.argv.slice(2));
  } catch (e) {
    fail(e.message);
  }
  try {
    const libdir = run("rustc", ["--print", "target-libdir", "--target", TARGET]);
    if (!existsSync(libdir)) throw new Error();
  } catch {
    fail(`the ${TARGET} target is missing: rustup target add ${TARGET}`);
  }
  const metadata = JSON.parse(run("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"]));
  let plugins = pluginPackages(metadata);
  if (args.names.length > 0) {
    const unknown = args.names.filter((n) => !plugins.some((p) => p.folder === n));
    if (unknown.length > 0) fail(`no plugin crate in plugins/${unknown.join(", plugins/")}`);
    plugins = plugins.filter((p) => args.names.includes(p.folder));
  }
  if (plugins.length === 0) fail("no plugin crates found in plugins/");
  const lld = rustLld();
  mkdirSync(args.outDir, { recursive: true });

  for (const plugin of plugins) {
    console.log(`build-plugins: ${plugin.name} (staticlib, profile ${PROFILE})`);
    const build = spawnSync(
      "cargo",
      [
        "rustc",
        "-p",
        plugin.name,
        "--lib",
        "--crate-type",
        "staticlib",
        "--target",
        TARGET,
        "--profile",
        PROFILE,
        "--locked",
        ...PROFILE_CONFIG.flatMap((c) => ["--config", c]),
      ],
      { cwd: ROOT, env: ENV, stdio: "inherit" },
    );
    if (build.status !== 0) fail(`cargo rustc of ${plugin.name} failed`);

    const staticlib = join(metadata.target_directory, TARGET, PROFILE, `lib${plugin.libName}.a`);
    const linked = join(args.outDir, `${plugin.folder}.linking.wasm`);
    const link = spawnSync(lld, linkArgs(staticlib, linked), { cwd: ROOT, env: ENV, stdio: "inherit" });
    if (link.status !== 0) fail(`linking ${plugin.name} failed`);

    const bytes = readFileSync(linked);
    const { manifest, problems } = checkPluginModule(bytes);
    const declared = JSON.parse(readFileSync(join(plugin.dir, "plugin.json"), "utf8"));
    if (manifest && JSON.stringify(manifest) !== JSON.stringify(declared)) {
      problems.push("the embedded manifest differs from plugin.json");
    }
    if (problems.length > 0) fail(`${plugin.name} breaks the plugin ABI:\n  ${problems.join("\n  ")}`);
    const output = join(args.outDir, `${manifest.id}.wasm`);
    writeFileSync(output, bytes);
    rmSync(linked);
    console.log(
      `build-plugins: wrote ${relative(ROOT, output) || output} (${(bytes.length / 1024).toFixed(0)} KiB, manifest ${manifest.id} ${manifest.version})`,
    );
  }
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) main();
