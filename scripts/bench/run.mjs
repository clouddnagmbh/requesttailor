// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The reproducible benchmark: RequestTailor and the other desktop API clients installed on
// this machine, measured the same way on the same machine. Writes
// bench/results/<date>-<os>.json and .md, and the results table of website/benchmark/.
// Nothing is installed, downloaded or sent anywhere. Method: bench/README.md.
//
// Usage: node scripts/bench/run.mjs [options]
//   --apps <list>          requesttailor,postman,insomnia,bruno,hoppscotch (default: all)
//   --runs <n>             starts per app, the first is discarded (default 10)
//   --memory-runs <n>      of those, the last n also measure idle memory (default 3)
//   --rt-binary <path>     RequestTailor release binary (default: <target>/release/rt-desktop)
//   --app <id>=<path>      where an app is (an .app bundle on macOS, else the executable)
//   --perf-results <file>  take RequestTailor's browser metrics (100 MB response, keystrokes)
//                          from a scripts/perf/run.mjs result instead of measuring them
//   --browser              measure those with scripts/perf/run.mjs --only frames,keystrokes
//   --out-dir <dir>        default bench/results
//   --no-website           do not update website/benchmark/index.html
//
// RequestTailor's 100 MB response and keystroke metrics are browser measurements of its UI;
// they have no counterpart in other apps without driving their UI, so they are n/a there.

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { arch, cpus, loadavg, platform, release, tmpdir, totalmem } from "node:os";
import { basename, dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { parseMarks, recentFile } from "../perf/desktop.mjs";
import { defaultBinary } from "../perf/run.mjs";
import { discover, electronIsolation } from "./apps.mjs";
import { measureApp } from "./launch.mjs";
import { RESULTS_DIR, budgetMaxima, markdown, replaceBlock, websiteBlock } from "./report.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const ALL = ["requesttailor", "postman", "insomnia", "bruno", "hoppscotch"];
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

export function parseArgs(argv) {
  const opts = { apps: ALL, runs: 10, memoryRuns: 3, overrides: {}, outDir: RESULTS_DIR, website: true, browser: false };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    const value = () => {
      const v = argv[++i];
      if (v === undefined) throw new Error(`${a} needs a value`);
      return v;
    };
    if (a === "--apps") {
      opts.apps = value().split(",").map((s) => s.trim());
      const unknown = opts.apps.filter((x) => !ALL.includes(x));
      if (unknown.length > 0) throw new Error(`unknown app(s): ${unknown.join(", ")} (known: ${ALL.join(", ")})`);
    } else if (a === "--runs") opts.runs = Number(value());
    else if (a === "--memory-runs") opts.memoryRuns = Number(value());
    else if (a === "--rt-binary") opts.rtBinary = resolve(value());
    else if (a === "--app") {
      const [id, ...path] = value().split("=");
      if (!ALL.includes(id) || id === "requesttailor" || path.length === 0) throw new Error(`--app needs <id>=<path> with id one of ${ALL.slice(1).join(", ")}`);
      opts.overrides[id] = path.join("=");
    } else if (a === "--perf-results") opts.perfResults = resolve(value());
    else if (a === "--browser") opts.browser = true;
    else if (a === "--out-dir") opts.outDir = resolve(value());
    else if (a === "--no-website") opts.website = false;
    else throw new Error(`unknown option ${a}`);
  }
  if (!Number.isInteger(opts.runs) || opts.runs < 2) throw new Error("--runs must be at least 2 (the first start is discarded)");
  if (!Number.isInteger(opts.memoryRuns) || opts.memoryRuns < 1 || opts.memoryRuns >= opts.runs) {
    throw new Error("--memory-runs must be at least 1 and less than --runs");
  }
  return opts;
}

/** `macos`, `linux` or `windows`: the file name part. */
export function osSlug(os = process.platform) {
  return { darwin: "macos", win32: "windows" }[os] ?? os;
}

function sh(cmd, args) {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return null;
  }
}

/** Hardware and OS, read from the OS (versions are part of the result). */
export function machine() {
  const os = platform();
  const info = { platform: os, arch: arch(), cpu: cpus()[0]?.model ?? "?", cpus: cpus().length, memoryGB: Math.round(totalmem() / 2 ** 30), kernel: release() };
  if (os === "darwin") {
    return { ...info, osName: "macOS", osVersion: `${sh("sw_vers", ["-productVersion"])} (${sh("sw_vers", ["-buildVersion"])})`, model: sh("sysctl", ["-n", "hw.model"]) };
  }
  if (os === "linux") {
    const pretty = (() => {
      try {
        return readFileSync("/etc/os-release", "utf8").match(/^PRETTY_NAME="?([^"\n]+)/m)?.[1];
      } catch {
        return null;
      }
    })();
    return { ...info, osName: "Linux", osVersion: pretty ?? release(), model: null };
  }
  return { ...info, osName: os === "win32" ? "Windows" : os, osVersion: release(), model: null };
}

/** macOS renders no frames for a locked screen (as in scripts/perf/run.mjs). */
function screenLocked() {
  if (process.platform !== "darwin") return false;
  const r = spawnSync("ioreg", ["-n", "Root", "-d1", "-a"], { encoding: "utf8" });
  return /<key>CGSSessionScreenIsLocked<\/key>\s*<true\/>/.test(r.stdout ?? "");
}

function running(prefix) {
  const r = spawnSync("ps", ["-axo", "comm="], { encoding: "utf8" });
  return (r.stdout ?? "").split("\n").some((l) => l.trim().startsWith(prefix));
}

function rtVersion() {
  const cargo = readFileSync(join(ROOT, "Cargo.toml"), "utf8");
  const version = cargo.match(/\[workspace\.package\][^[]*?version\s*=\s*"([^"]+)"/)?.[1] ?? "?";
  const commit = sh("git", ["-C", ROOT, "rev-parse", "--short", "HEAD"]);
  const dirty = sh("git", ["-C", ROOT, "status", "--porcelain", "--untracked-files=no"]);
  return { version, commit: commit ? `${commit}${dirty ? "+changes" : ""}` : null };
}

async function waitForMarks(file, names, timeoutMs) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    if (existsSync(file)) {
      const marks = parseMarks(readFileSync(file, "utf8"));
      if (names.every((n) => n in marks)) return marks;
    }
    await sleep(5);
  }
  throw new Error(`timed out waiting for marks ${names.join(", ")}`);
}

/** RequestTailor: the release binary in measurement mode (perf/README.md), so each start
 *  also yields the internal mark `first-frame`, the `cold_start` budget's definition. */
async function measureRequestTailor(opts, log) {
  const binary = opts.rtBinary ?? defaultBinary();
  const { version, commit } = rtVersion();
  const app = { id: "requesttailor", name: "RequestTailor", installed: true, version, commit, binary, notApplicable: {}, notes: [] };
  if (!existsSync(binary)) return { ...app, error: `release binary not found: ${binary} (build it: npm run build -w apps/desktop)` };
  app.binaryBuilt = statSync(binary).mtime.toISOString();
  const collection = join(ROOT, "testdata", "conformance", "collection");
  // Its own data folder for the series: the user's plugins and settings stay out of it.
  const data = mkdtempSync(join(tmpdir(), "rt-bench-data-"));
  try {
    const r = await measureApp({
      executable: binary,
      bundle: binary,
      runs: opts.runs,
      memoryRuns: opts.memoryRuns,
      log,
      hooks: {
        prepare: () => {
          const dir = mkdtempSync(join(tmpdir(), "rt-bench-perf-"));
          writeFileSync(join(dir, "recent-collections.json"), recentFile(collection));
          return {
            envAt: (t0) => ({ RT_PERF_DIR: dir, RT_PERF_T0: t0.toFixed(3), RT_DATA_DIR: data }),
            after: async () => {
              const marks = await waitForMarks(join(dir, "marks.log"), ["first-frame", "collection-open"], 60_000);
              return { firstFrameMs: marks["first-frame"], collectionOpenMs: marks["collection-open"] };
            },
            cleanup: () => rmSync(dir, { recursive: true, force: true }),
          };
        },
      },
    });
    app.results = { first_window: r.first_window, cold_start: r.cold_start, idle_memory: r.idle_memory, idle_memory_rss: r.idle_memory_rss };
    app.samples = r.samples;
    app.notes.push("first interactive frame: internal mark of the release build (perf/README.md); idle memory with the sample collection open");
  } catch (e) {
    app.error = e.message;
  } finally {
    rmSync(data, { recursive: true, force: true });
  }
  return app;
}

function browserMetrics(opts, app, log) {
  let file = opts.perfResults;
  let scratch = null;
  if (!file && opts.browser) {
    scratch = mkdtempSync(join(tmpdir(), "rt-bench-browser-"));
    file = join(scratch, "results.json");
    log("$ node scripts/perf/run.mjs --only frames,keystrokes");
    spawnSync(process.execPath, [join(ROOT, "scripts", "perf", "run.mjs"), "--only", "frames,keystrokes", "--out", file], { cwd: ROOT, stdio: "inherit" });
  }
  if (!file) return;
  try {
    const perf = JSON.parse(readFileSync(file, "utf8"));
    for (const k of ["large_response_frame", "send_keystrokes"]) {
      if (typeof perf[k] === "number") app.results[k] = perf[k];
    }
    app.notes.push(`100 MB response and keystrokes: scripts/perf/run.mjs in headless Chrome (${opts.perfResults ? basename(file) : "this run"})`);
  } catch (e) {
    app.notes.push(`browser metrics not read: ${e.message}`);
  } finally {
    if (scratch) rmSync(scratch, { recursive: true, force: true });
  }
}

/** A competitor: fresh profile (Electron), offline renderer, no account. */
async function measureCompetitor(found, opts, log) {
  const app = { id: found.id, name: found.name, installed: found.installed, notApplicable: { cold_start: true, large_response_frame: true, send_keystrokes: true }, notes: [] };
  if (!found.installed) return { ...app, searched: found.searched };
  Object.assign(app, { version: found.version, path: found.path, engine: found.engine });
  if (running(found.path)) return { ...app, error: `${found.name} is running; quit it first (the benchmark never stops apps it did not start)` };
  const profile = mkdtempSync(join(tmpdir(), `rt-bench-${found.id}-`));
  try {
    const args = found.engine === "electron" ? electronIsolation(profile) : [];
    const r = await measureApp({ executable: found.executable, bundle: found.path, engine: found.engine, args, runs: opts.runs, memoryRuns: opts.memoryRuns, log });
    app.results = { first_window: r.first_window, idle_memory: r.idle_memory, idle_memory_rss: r.idle_memory_rss };
    app.samples = r.samples;
    app.notes.push(
      found.engine === "electron"
        ? "fresh profile (--user-data-dir) shared by the starts of the series, renderer offline (--proxy-server to a closed port), no account: the first window is the app's start screen without sign-in"
        : "started with its own default profile (the app has no option for another one)",
    );
  } catch (e) {
    app.error = e.message;
  } finally {
    rmSync(profile, { recursive: true, force: true });
  }
  return app;
}

async function main() {
  const opts = parseArgs(process.argv.slice(2));
  const log = console.log;
  if (screenLocked()) throw new Error("the screen is locked: macOS renders no frames for a locked screen. Unlock and run again.");
  const load = loadavg().map((l) => Math.round(l * 10) / 10);
  if (load[0] > cpus().length) log(`warning: load average ${load[0]} on ${cpus().length} CPUs; timings will be pessimistic`);
  const result = {
    version: 1,
    date: new Date().toISOString(),
    machine: machine(),
    node: process.version,
    loadAverage: load,
    method: {
      runs: opts.runs,
      memoryRuns: opts.memoryRuns,
      discarded: 1,
      statistic: "median",
      idle: "at least 3 s after the first window (RequestTailor: after the collection opened), then until three samples one second apart differ by less than 2 % (at most 20 s)",
      firstWindow: "process spawn to the first on-screen window of the app's process, at least 100x100 points (macOS: CoreGraphics window list, polled every 5 ms)",
      memory: "perf/README.md: macOS phys_footprint, Linux PSS, Windows private working set; app and helper/web view processes",
    },
    apps: [],
  };
  const competitors = discover({ overrides: opts.overrides });
  for (const id of opts.apps) {
    log(`\n== ${id}`);
    let app;
    if (id === "requesttailor") {
      app = await measureRequestTailor(opts, log);
      if (!app.error) browserMetrics(opts, app, log);
    } else {
      app = await measureCompetitor(competitors.find((c) => c.id === id), opts, log);
      if (!app.installed) log(`  not installed (looked in ${app.searched.join(", ")})`);
    }
    if (app.error) log(`  FAILED: ${app.error}`);
    result.apps.push(app);
  }
  mkdirSync(opts.outDir, { recursive: true });
  const stem = `${result.date.slice(0, 10)}-${osSlug()}`;
  const json = join(opts.outDir, `${stem}.json`);
  const budgets = budgetMaxima(readFileSync(join(ROOT, "perf", "budgets.toml"), "utf8"));
  writeFileSync(json, `${JSON.stringify(result, null, 2)}\n`);
  writeFileSync(join(opts.outDir, `${stem}.md`), markdown(result, budgets));
  log(`\nwrote ${json} and ${stem}.md`);
  if (opts.website) {
    const page = join(ROOT, "website", "benchmark", "index.html");
    writeFileSync(page, replaceBlock(readFileSync(page, "utf8"), websiteBlock(result, budgets)));
    log(`updated ${page}`);
  }
  process.stdout.write(`\n${markdown(result, budgets)}`);
  if (result.apps.some((a) => a.error)) process.exit(1);
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  main().catch((e) => {
    console.error(e.message);
    process.exit(1);
  });
}
