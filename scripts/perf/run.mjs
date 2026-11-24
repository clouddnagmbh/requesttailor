// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Runs the performance measurements on this machine and writes perf/results.json (the numbers
// scripts/check-perf-budgets.mjs reads) and perf/report.json (how they came about). Nothing
// is sent anywhere. What is measured and how: perf/README.md.
//
// Usage: node scripts/perf/run.mjs [options]
//   --only <list>       comma-separated parts: desktop, frames, keystrokes, diff, graph, compare,
//                       status, graph-large, snapshots, share, observe, watch,
//                       envcompare, checks, paging, ratelimit, inbox, lint, views, schemas,
//                       catalogue, trace, egress, journal, access, shrink, consumers, sequence,
//                       security, notebook, debugger (default: all)
//   --binary <path>     desktop release binary (default: <target dir>/release/rt-desktop)
//   --collection <dir>  collection the desktop app reopens (default: testdata/conformance/collection)
//   --collections <n>   copies of it in one folder that the app reopens (default 10, wave 14;
//                       1: the collection alone)
//   --runs <n>          desktop starts, the first is discarded (default 10)
//   --memory-runs <n>   of those, the last n also measure idle memory (default 3)
//   --out <file>        results file (default perf/results.json; the report goes next to it)
//   --check             afterwards run the budget check (with --require-all unless --only)
//
// Needs: the desktop release build (`npm run build -w apps/desktop`) for `desktop`; Google
// Chrome, the wasm toolchain (scripts/build-wasm.mjs) and cargo for `frames` and `keystrokes`.
// Linux without a display: run under `xvfb-run -a`.

import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { arch, cpus, loadavg, platform, release, tmpdir, totalmem } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { measureCompare } from "./compare.mjs";
import { measureDesktop } from "./desktop.mjs";
import { measureGraph } from "./graph.mjs";
import { measureGraphLarge } from "./graph-large.mjs";
import { measureObserve } from "./observe.mjs";
import { measureShare } from "./share.mjs";
import { measureSnapshots } from "./snapshots.mjs";
import { measureStatus } from "./status.mjs";
import { measureWatch } from "./watch.mjs";
import { measureEnvCompare } from "./envcompare.mjs";
import { measureChecks } from "./checks.mjs";
import { measurePaging } from "./paging.mjs";
import { measureRateLimit } from "./ratelimit.mjs";
import { measureInbox } from "./inbox.mjs";
import { measureLint } from "./lint.mjs";
import { measureViews } from "./views.mjs";
import { measureSchemas } from "./schemas.mjs";
import { measureCatalogue } from "./catalogue.mjs";
import { measureTrace } from "./trace.mjs";
import { measureEgress } from "./egress.mjs";
import { measureJournal } from "./journal.mjs";
import { measureAccess } from "./access.mjs";
import { measureShrink } from "./shrink.mjs";
import { measureConsumers } from "./consumers.mjs";
import { measureSequence } from "./sequence.mjs";
import { measureSecurity } from "./security.mjs";
import { measureNotebook } from "./notebook.mjs";
import { measureDebugger } from "./debugger.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const WEB = join(ROOT, "apps", "web");
const PARTS = ["desktop", "frames", "keystrokes", "diff", "graph", "compare", "status", "graph-large", "snapshots", "share", "observe", "watch", "envcompare", "checks", "paging", "ratelimit", "inbox", "lint", "views", "schemas", "catalogue", "trace", "egress", "journal", "access", "shrink", "consumers", "sequence", "security", "notebook", "debugger"];

export function parseArgs(argv) {
  const opts = { only: PARTS, runs: 10, memoryRuns: 3, check: false, out: join(ROOT, "perf", "results.json") };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${a} needs a value`);
      return v;
    };
    if (a === "--only") {
      opts.only = value().split(",").map((s) => s.trim());
      const unknown = opts.only.filter((p) => !PARTS.includes(p));
      if (unknown.length > 0) throw new Error(`unknown part(s): ${unknown.join(", ")} (known: ${PARTS.join(", ")})`);
      opts.partial = true;
    } else if (a === "--binary") opts.binary = resolve(value());
    else if (a === "--collection") opts.collection = resolve(value());
    else if (a === "--collections") opts.collections = Number(value());
    else if (a === "--runs") opts.runs = Number(value());
    else if (a === "--memory-runs") opts.memoryRuns = Number(value());
    else if (a === "--out") opts.out = resolve(value());
    else if (a === "--check") opts.check = true;
    else throw new Error(`unknown option ${a}`);
  }
  if (opts.collections !== undefined && (!Number.isInteger(opts.collections) || opts.collections < 1)) {
    throw new Error("--collections must be at least 1");
  }
  if (!Number.isInteger(opts.runs) || opts.runs < 2) throw new Error("--runs must be at least 2 (the first start is discarded)");
  if (!Number.isInteger(opts.memoryRuns) || opts.memoryRuns < 1 || opts.memoryRuns >= opts.runs) {
    throw new Error("--memory-runs must be at least 1 and less than --runs");
  }
  return opts;
}

/** Default location of the desktop release binary (CARGO_TARGET_DIR is honoured). */
export function defaultBinary(env = process.env, os = process.platform) {
  const target = env.CARGO_TARGET_DIR ? resolve(env.CARGO_TARGET_DIR) : join(ROOT, "target");
  return join(target, "release", os === "win32" ? "rt-desktop.exe" : "rt-desktop");
}

/** The p95 the budget sees: the worst of the measured variants (mock UI, real core). */
export function worstP95(runs) {
  const values = runs.map((r) => r.p95).filter((v) => typeof v === "number" && Number.isFinite(v));
  return values.length === runs.length && values.length > 0 ? Math.max(...values) : undefined;
}

/** macOS renders no frames for a locked screen; then the cold start cannot be measured. */
function screenLocked() {
  if (process.platform !== "darwin") return false;
  const r = spawnSync("ioreg", ["-n", "Root", "-d1", "-a"], { encoding: "utf8" });
  return /<key>CGSSessionScreenIsLocked<\/key>\s*<true\/>/.test(r.stdout ?? "");
}

function run(cmd, args, opts = {}) {
  console.log(`$ ${cmd} ${args.join(" ")}`);
  const r = spawnSync(cmd, args, { stdio: "inherit", shell: process.platform === "win32", ...opts });
  if (r.status !== 0) throw new Error(`${cmd} ${args.join(" ")} failed (exit ${r.status ?? r.signal})`);
}

function readJson(file) {
  return JSON.parse(readFileSync(file, "utf8"));
}

async function desktop(opts) {
  if (screenLocked()) {
    throw new Error("the screen is locked: macOS renders no frames for the app's window, so its first frame never comes. Unlock and run again.");
  }
  const r = await measureDesktop({
    binary: opts.binary ?? defaultBinary(),
    collection: opts.collection ?? join(ROOT, "testdata", "conformance", "collection"),
    collections: opts.collections ?? 10,
    runs: opts.runs,
    memoryRuns: opts.memoryRuns,
  });
  return { results: { cold_start: r.cold_start, idle_memory: r.idle_memory }, details: r.details };
}

function playwright(outDir, args, env = {}) {
  run("npx", ["playwright", "test", ...args], { cwd: WEB, env: { ...process.env, RT_PERF_OUT: outDir, ...env } });
}

async function frames() {
  run("node", [join(ROOT, "scripts", "build-wasm.mjs")]);
  const out = mkdtempSync(join(tmpdir(), "rt-perf-frames-"));
  try {
    playwright(out, ["--config", "e2e/perf/playwright.perf.config.ts"]);
    const mock = readJson(join(out, "frames-mock.json"));
    const core = readJson(join(out, "frames-core.json"));
    return { results: { large_response_frame: worstP95([mock, core]) }, details: { mock, core } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

async function keystrokes() {
  run("node", [join(ROOT, "scripts", "build-wasm.mjs")]);
  const out = mkdtempSync(join(tmpdir(), "rt-perf-keys-"));
  try {
    // Mock backend (the base config's dev server, on its own port) and the real web backend.
    playwright(out, ["e2e/recent/recent.spec.ts", "-g", "keystroke budget"], { RT_WEB_PORT: process.env.RT_WEB_PORT ?? "47845" });
    playwright(out, ["--config", "e2e/recent/playwright.recent.config.ts", "-g", "keystroke budget"]);
    const mock = readJson(join(out, "keystrokes-mock.json")).send_keystrokes;
    const web = readJson(join(out, "keystrokes-web.json")).send_keystrokes;
    return { results: { send_keystrokes: Math.max(mock, web) }, details: { mock, web } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

/** Wave 12: the semantic diff of 1000 requests (crates/rt-format/tests/diff_perf.rs), release build. */
async function diff() {
  const out = mkdtempSync(join(tmpdir(), "rt-perf-diff-"));
  try {
    run("cargo", ["test", "--release", "--locked", "-p", "rt-format", "--test", "diff_perf", "--", "semantic_diff_of_1000_requests_is_measured", "--exact", "--nocapture"], {
      cwd: ROOT,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const r = readJson(join(out, "diff.json"));
    return { results: { semantic_diff: r.semantic_diff }, details: { runs: r.runs } };
  } finally {
    rmSync(out, { recursive: true, force: true });
  }
}

/** Wave 14, strand F: the commit graph of 10 000 commits (./graph.mjs). */
async function graph(opts) {
  return measureGraph({ root: ROOT, web: WEB, run, readJson, opts });
}

/** Wave 14, strand H: a comparison of 1000 requests (./compare.mjs). */
async function compare(opts) {
  return measureCompare({ root: ROOT, web: WEB, run, readJson, opts });
}

/** Wave 15, strand B: the working copy of 50 000 files (./status.mjs). */
async function status(opts) {
  return measureStatus({ root: ROOT, run, readJson, opts });
}

/** Wave 15, strand C: the commit graph of 100 000 commits (./graph-large.mjs). */
async function graphLarge(opts) {
  return measureGraphLarge({ root: ROOT, web: WEB, run, readJson, opts });
}

/** Wave 17: snapshot comparison (strand B, ./snapshots.mjs), share links (C, ./share.mjs),
 *  observer overhead (A, ./observe.mjs), `rt send --watch` (D, ./watch.mjs). */
async function snapshots(opts) {
  return measureSnapshots({ root: ROOT, run, readJson, opts });
}

async function share(opts) {
  return measureShare({ root: ROOT, web: WEB, run, readJson, opts });
}

async function observe(opts) {
  return measureObserve({ root: ROOT, run, readJson, opts });
}

async function watch(opts) {
  return measureWatch({ root: ROOT, run, readJson, opts });
}

/** Wave 18: one part per strand (A to I), each in its own file; placeholders until the strand. */
async function envcompare(opts) {
  return measureEnvCompare({ root: ROOT, web: WEB, run, readJson, opts });
}

async function checks(opts) {
  return measureChecks({ root: ROOT, web: WEB, run, readJson, opts });
}

async function paging(opts) {
  return measurePaging({ root: ROOT, web: WEB, run, readJson, opts });
}

async function ratelimit(opts) {
  return measureRateLimit({ root: ROOT, web: WEB, run, readJson, opts });
}

async function inbox(opts) {
  return measureInbox({ root: ROOT, web: WEB, run, readJson, opts });
}

async function lint(opts) {
  return measureLint({ root: ROOT, web: WEB, run, readJson, opts });
}

async function views(opts) {
  return measureViews({ root: ROOT, web: WEB, run, readJson, opts });
}

async function schemas(opts) {
  return measureSchemas({ root: ROOT, web: WEB, run, readJson, opts });
}

async function catalogue(opts) {
  return measureCatalogue({ root: ROOT, web: WEB, run, readJson, opts });
}

/** Wave 19: one part per strand, each in its own file; placeholders until the strand. */
async function trace(opts) {
  return measureTrace({ root: ROOT, web: WEB, run, readJson, opts });
}

async function egress(opts) {
  return measureEgress({ root: ROOT, web: WEB, run, readJson, opts });
}

async function journal(opts) {
  return measureJournal({ root: ROOT, web: WEB, run, readJson, opts });
}

async function access(opts) {
  return measureAccess({ root: ROOT, web: WEB, run, readJson, opts });
}

async function shrink(opts) {
  return measureShrink({ root: ROOT, web: WEB, run, readJson, opts });
}

async function consumers(opts) {
  return measureConsumers({ root: ROOT, web: WEB, run, readJson, opts });
}

async function sequence(opts) {
  return measureSequence({ root: ROOT, web: WEB, run, readJson, opts });
}

async function security(opts) {
  return measureSecurity({ root: ROOT, web: WEB, run, readJson, opts });
}

async function notebook(opts) {
  return measureNotebook({ root: ROOT, web: WEB, run, readJson, opts });
}

async function debuggerPart(opts) {
  return measureDebugger({ root: ROOT, web: WEB, run, readJson, opts });
}

const STEPS = {
  desktop,
  frames,
  keystrokes,
  diff,
  graph,
  compare,
  status,
  "graph-large": graphLarge,
  snapshots,
  share,
  observe,
  watch,
  envcompare,
  checks,
  paging,
  ratelimit,
  inbox,
  lint,
  views,
  schemas,
  catalogue,
  trace,
  egress,
  journal,
  access,
  shrink,
  consumers,
  sequence,
  security,
  notebook,
  debugger: debuggerPart,
};

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const results = {};
  const report = {
    date: new Date().toISOString(),
    machine: { platform: platform(), release: release(), arch: arch(), cpu: cpus()[0]?.model ?? "?", cpus: cpus().length, memoryGB: Math.round(totalmem() / 2 ** 30) },
    node: process.version,
    // 1, 5 and 15 minute load averages at the start (0 on Windows): a busy machine measures slow.
    loadAverage: loadavg().map((l) => Math.round(l * 10) / 10),
    parts: {},
    errors: {},
  };
  if (loadavg()[0] > cpus().length) {
    console.warn(`warning: load average ${loadavg()[0].toFixed(1)} on ${cpus().length} CPUs; timings will be pessimistic`);
  }
  for (const part of opts.only) {
    console.log(`\n== ${part}`);
    try {
      const r = await STEPS[part](opts);
      for (const [k, v] of Object.entries(r.results)) if (typeof v === "number") results[k] = v;
      report.parts[part] = r;
      console.log(`${part}: ${JSON.stringify(r.results)}`);
    } catch (e) {
      report.errors[part] = e.message;
      console.error(`${part} FAILED: ${e.message}`);
    }
  }
  mkdirSync(dirname(opts.out), { recursive: true });
  writeFileSync(opts.out, `${JSON.stringify(results, null, 2)}\n`);
  const reportFile = join(dirname(opts.out), "report.json");
  writeFileSync(reportFile, `${JSON.stringify(report, null, 2)}\n`);
  console.log(`\nwrote ${opts.out} and ${reportFile}`);

  let failed = Object.keys(report.errors).length > 0;
  if (opts.check) {
    const args = [join(ROOT, "scripts", "check-perf-budgets.mjs"), opts.out];
    if (!opts.partial) args.push("--require-all");
    const r = spawnSync("node", args, { cwd: ROOT, stdio: "inherit" });
    failed ||= r.status !== 0;
  }
  if (failed) process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
