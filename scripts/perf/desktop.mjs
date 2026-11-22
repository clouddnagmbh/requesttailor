// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Desktop measurements (budgets `cold_start` and `idle_memory`, perf/README.md): starts the
// release binary N times in measurement mode and reads the marks the app writes.
//
// Measurement mode: `RT_PERF_DIR` (a fresh temporary folder per run) holds the recent list
// that makes the app reopen the sample collection, and receives `marks.log`; `RT_PERF_T0` is
// the Unix time in ms taken right before the process is spawned. Marks:
//   first-frame      the UI asked for the recent list, which it does in the first task after
//                    its first animation frame (apps/ui/src/recent/startup.ts): the shell is
//                    painted and its main thread accepts input. This is `cold_start`.
//   collection-open  the sample collection is open in the core (reopened from the list).

import { spawn, spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, join } from "node:path";

import { baseline, sample } from "./memory.mjs";

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** `marks.log` text → { name: ms }. */
export function parseMarks(text) {
  const marks = {};
  for (const line of text.split("\n")) {
    const m = line.trim().match(/^([a-z][a-z0-9-]*)\s+(-?\d+(?:\.\d+)?)$/);
    if (m && !(m[1] in marks)) marks[m[1]] = Number(m[2]);
  }
  return marks;
}

export function median(values) {
  if (values.length === 0) return NaN;
  const s = [...values].sort((a, b) => a - b);
  const mid = Math.floor(s.length / 2);
  return s.length % 2 === 1 ? s[mid] : (s[mid - 1] + s[mid]) / 2;
}

/** Memory is idle when the last `window` samples differ by less than `tolerance` (relative). */
export function isSettled(totals, window = 3, tolerance = 0.02) {
  if (totals.length < window) return false;
  const last = totals.slice(-window);
  const lo = Math.min(...last);
  const hi = Math.max(...last);
  return hi - lo <= tolerance * hi;
}

/** The recent list that makes the app reopen `collection` at start. */
export function recentFile(collection) {
  const id = realpathSync.native(collection).replace(/^\\\\\?\\/, "");
  return JSON.stringify({ version: 1, collections: [{ id, name: basename(collection), location: id, openedAt: Date.now() }] }, null, 2);
}

async function waitForMarks(file, names, child, timeoutMs) {
  const until = Date.now() + timeoutMs;
  while (Date.now() < until) {
    if (child.exitCode !== null) throw new Error(`the app exited early (code ${child.exitCode})`);
    if (existsSync(file)) {
      const marks = parseMarks(readFileSync(file, "utf8"));
      if (names.every((n) => n in marks)) return marks;
    }
    await sleep(5);
  }
  const seen = existsSync(file) ? readFileSync(file, "utf8").trim() : "nothing";
  throw new Error(`timed out waiting for marks ${names.join(", ")} (got: ${seen || "nothing"})`);
}

function stop(child) {
  if (child.exitCode !== null) return Promise.resolve();
  const exited = new Promise((r) => child.once("exit", r));
  if (process.platform === "win32") {
    // The whole tree: WebView2 processes are children of the app.
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill("SIGTERM");
  }
  return Promise.race([exited, sleep(5000).then(() => child.kill("SIGKILL"))]);
}

/** One start of the app. With `memory`, also waits for idle and samples memory. */
async function runOnce({ binary, collection, memory, idle }) {
  const dir = mkdtempSync(join(tmpdir(), "rt-perf-"));
  writeFileSync(join(dir, "recent-collections.json"), recentFile(collection));
  const marksFile = join(dir, "marks.log");
  const before = baseline();
  const t0 = performance.timeOrigin + performance.now();
  const child = spawn(binary, [], {
    env: { ...process.env, RT_PERF_DIR: dir, RT_PERF_T0: t0.toFixed(3) },
    stdio: ["ignore", "ignore", "pipe"],
  });
  let stderr = "";
  child.stderr.on("data", (d) => (stderr = (stderr + d).slice(-4000)));
  try {
    const marks = await waitForMarks(marksFile, ["first-frame", "collection-open"], child, 60_000);
    const result = {
      coreReadyMs: marks["core-ready"] ?? NaN,
      firstFrameMs: marks["first-frame"],
      collectionOpenMs: marks["collection-open"],
    };
    if (memory) {
      // Idle: at least `minMs` after the collection opened, then until memory is stable.
      await sleep(idle.minMs);
      const totals = [];
      let last = null;
      const until = Date.now() + idle.maxMs;
      while (Date.now() < until) {
        last = sample(child.pid, before);
        totals.push(last.ownMB);
        if (isSettled(totals)) break;
        await sleep(idle.intervalMs);
      }
      result.memory = { ...last, settled: isSettled(totals), samples: totals };
    }
    return result;
  } catch (e) {
    throw new Error(`${e.message}${stderr ? `\napp stderr:\n${stderr}` : ""}`);
  } finally {
    await stop(child);
    rmSync(dir, { recursive: true, force: true });
  }
}

/** Starts the app `runs` times (the first is a warm-up and discarded); memory in the last
 *  `memoryRuns`. Returns the medians and every sample. */
export async function measureDesktop({
  binary,
  collection,
  runs = 10,
  memoryRuns = 3,
  idle = { minMs: 3000, maxMs: 20_000, intervalMs: 1000 },
  log = console.log,
}) {
  if (!existsSync(binary)) throw new Error(`desktop binary not found: ${binary} (build it: npm run build -w apps/desktop)`);
  const starts = [];
  for (let i = 0; i < runs; i++) {
    const memory = i >= runs - memoryRuns;
    const r = await runOnce({ binary, collection, memory, idle });
    const mem = r.memory ? `, memory ${r.memory.ownMB} MB (rss ${r.memory.rssMB} MB)` : "";
    log(`  run ${i + 1}/${runs}${i === 0 ? " (warm-up, discarded)" : ""}: core ready ${r.coreReadyMs.toFixed(0)} ms, first frame ${r.firstFrameMs.toFixed(0)} ms, collection open ${r.collectionOpenMs.toFixed(0)} ms${mem}`);
    if (r.memory?.warning) log(`    warning: ${r.memory.warning}`);
    starts.push(r);
    // Let the previous run's webview processes go away.
    await sleep(500);
  }
  const counted = starts.slice(1);
  const withMemory = starts.filter((s) => s.memory);
  const byOwn = [...withMemory].sort((a, b) => a.memory.ownMB - b.memory.ownMB);
  const medianMemory = byOwn[Math.floor((byOwn.length - 1) / 2)]?.memory ?? null;
  return {
    cold_start: Math.round(median(counted.map((s) => s.firstFrameMs))),
    idle_memory: medianMemory ? medianMemory.ownMB : undefined,
    details: {
      runs,
      firstFrameMs: counted.map((s) => Math.round(s.firstFrameMs)),
      collectionOpenMs: counted.map((s) => Math.round(s.collectionOpenMs)),
      collectionOpenMedianMs: Math.round(median(counted.map((s) => s.collectionOpenMs))),
      memory: medianMemory,
      memoryRunsMB: withMemory.map((s) => s.memory.ownMB),
    },
  };
}
