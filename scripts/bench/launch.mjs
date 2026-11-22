// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Starts a desktop app N times and measures, the same way for every app:
//   firstWindowMs  process spawn → the app's first on-screen window (macOS: window-mac.js,
//                  CoreGraphics' window list; Windows: MainWindowHandle; Linux: xdotool if
//                  installed). An app that shows its window before its content is favoured
//                  by this, never penalised.
//   memory         idle memory of the app and its helper/webview processes with the
//                  definitions of perf/README.md (scripts/perf/memory.mjs): at least 3 s
//                  after the first window, then until three samples differ by < 2 %.
// The first start is a warm-up and discarded (code-signing checks, cold disk caches), as in
// scripts/perf/desktop.mjs. `hooks` add per-app setup (RequestTailor's measurement mode and
// its internal marks).

import { execFileSync, spawn, spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { isSettled, median } from "../perf/desktop.mjs";
import { baseline, descendants, parseFootprint, parsePs, sample, summarize } from "../perf/memory.mjs";

const HERE = dirname(fileURLToPath(import.meta.url));
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function processes() {
  const r = spawnSync("ps", ["-axo", "pid=,ppid=,rss=,comm="], { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 });
  return parsePs(r.stdout ?? "");
}

/** Pids of processes started for this run: the child's tree, and (macOS) processes from the
 *  app's bundle that did not run before (Electron's crashpad handler is re-parented to
 *  launchd). Only these are ever stopped. */
export function ownProcesses(procs, root, before, bundle) {
  const tree = descendants(procs, root);
  const extra = bundle ? procs.filter((p) => !before.has(p.pid) && p.name.startsWith(bundle)).map((p) => p.pid) : [];
  return [...new Set([...tree, ...extra])].filter((pid) => procs.some((p) => p.pid === pid));
}

/** Watches for the first window of the process whose pid is written to `pidFile`. Resolves
 *  `{ ready }` once it polls; `window` resolves with the epoch ms of the first window. */
function watchWindow(pidFile, timeoutMs) {
  if (process.platform === "darwin") {
    const child = spawn("osascript", ["-l", "JavaScript", join(HERE, "window-mac.js"), pidFile, String(timeoutMs)], {
      stdio: ["ignore", "ignore", "pipe"],
    });
    let buf = "";
    let onReady;
    let onWindow;
    const ready = new Promise((r) => (onReady = r));
    const window = new Promise((r) => (onWindow = r));
    child.stderr.on("data", (d) => {
      buf += d;
      for (const line of buf.split("\n")) {
        const m = line.match(/^(ready|window) (\d+)/);
        if (m?.[1] === "ready") onReady();
        if (m?.[1] === "window") onWindow(Number(m[2]));
        if (line.startsWith("timeout")) onWindow(null);
      }
    });
    child.on("exit", () => {
      onReady();
      onWindow(null);
    });
    return { ready, window, stop: () => child.kill() };
  }
  // Windows and Linux: poll from here.
  let stopped = false;
  const window = (async () => {
    const until = Date.now() + timeoutMs;
    while (!stopped && Date.now() < until) {
      const pid = Number(readFileSync(pidFile, "utf8")) || 0;
      if (pid > 0 && hasWindowElsewhere(pid)) return Date.now();
      await sleep(20);
    }
    return null;
  })();
  return { ready: Promise.resolve(), window, stop: () => (stopped = true) };
}

function hasWindowElsewhere(pid) {
  if (process.platform === "win32") {
    const r = spawnSync("powershell", ["-NoProfile", "-NonInteractive", "-Command", `(Get-Process -Id ${pid}).MainWindowHandle -ne 0`], { encoding: "utf8" });
    return /True/.test(r.stdout ?? "");
  }
  const r = spawnSync("xdotool", ["search", "--onlyvisible", "--pid", String(pid)], { encoding: "utf8" });
  if (r.error) throw new Error("the first window on Linux needs xdotool (not installed)");
  return r.status === 0 && r.stdout.trim() !== "";
}

/** Memory of exactly `pids` (macOS: phys_footprint, as scripts/perf/memory.mjs does). For
 *  Electron apps on macOS: their windows render in their own helpers, so WebKit processes
 *  that other apps start meanwhile must not be counted. */
function sampleOwn(pids) {
  const procs = processes().filter((p) => pids.includes(p.pid));
  const text = execFileSync("footprint", ["--noCategories", "-f", "bytes", ...procs.map((p) => String(p.pid))], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
    maxBuffer: 64 * 1024 * 1024,
  });
  const footprints = parseFootprint(text);
  return { ...summarize(procs.map((p) => ({ pid: p.pid, name: p.name, rss: p.rssBytes, own: footprints.get(p.pid) ?? p.rssBytes }))), warning: null };
}

async function stopAll(child, procs) {
  const pids = procs();
  for (const pid of pids) {
    try {
      process.kill(pid, "SIGTERM");
    } catch {
      // Gone.
    }
  }
  const until = Date.now() + 5000;
  while (Date.now() < until && procs().length > 0) await sleep(100);
  for (const pid of procs()) {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // Gone.
    }
  }
  if (child.exitCode === null) await Promise.race([new Promise((r) => child.once("exit", r)), sleep(2000)]);
}

/**
 * Measures `runs` starts of `executable`. `hooks.prepare(i)` may return `{ args, env, after }`
 * per start; `after()` runs once the window is up and may return extra numbers (marks).
 * `bundle` (macOS): the app bundle path, so processes it re-parents are found and stopped.
 * `engine`: `webkit` (RequestTailor, Tauri: the app's tree plus the WebKit XPC services that
 * appeared, scripts/perf/memory.mjs) or `electron` (macOS: the processes of the bundle that
 * were started for this run, the re-parented crashpad handler included; elsewhere the tree).
 */
export async function measureApp({
  executable,
  bundle = null,
  args = [],
  env = {},
  runs = 10,
  memoryRuns = 3,
  idle = { minMs: 3000, maxMs: 20_000, intervalMs: 1000 },
  timeoutMs = 60_000,
  hooks = {},
  engine = "webkit",
  log = console.log,
}) {
  const starts = [];
  const scratch = mkdtempSync(join(tmpdir(), "rt-bench-"));
  try {
    for (let i = 0; i < runs; i++) {
      const memory = i >= runs - memoryRuns;
      const extra = (await hooks.prepare?.(i)) ?? {};
      const pidFile = join(scratch, `pid-${i}`);
      writeFileSync(pidFile, "");
      const watcher = watchWindow(pidFile, timeoutMs);
      await watcher.ready;
      const beforeProcs = new Set(processes().map((p) => p.pid));
      const before = baseline();
      const t0 = performance.timeOrigin + performance.now();
      const child = spawn(executable, [...args, ...(extra.args ?? [])], {
        env: { ...process.env, ...env, ...(extra.env ?? {}), ...(extra.envAt?.(t0) ?? {}) },
        stdio: ["ignore", "ignore", "ignore"],
        detached: false,
      });
      writeFileSync(pidFile, String(child.pid));
      const mine = () => ownProcesses(processes(), child.pid, beforeProcs, bundle);
      try {
        const at = await watcher.window;
        if (at === null) throw new Error(`no window within ${timeoutMs} ms${child.exitCode !== null ? ` (exited with ${child.exitCode})` : ""}`);
        const result = { firstWindowMs: at - t0 };
        Object.assign(result, (await extra.after?.()) ?? {});
        if (memory) {
          await sleep(idle.minMs);
          const totals = [];
          let last = null;
          const until = Date.now() + idle.maxMs;
          while (Date.now() < until) {
            last = engine === "electron" && process.platform === "darwin" ? sampleOwn(mine()) : sample(child.pid, before);
            totals.push(last.ownMB);
            if (isSettled(totals)) break;
            await sleep(idle.intervalMs);
          }
          result.memory = { ...last, settled: isSettled(totals), samples: totals };
        }
        const mem = result.memory ? `, memory ${result.memory.ownMB} MB (rss ${result.memory.rssMB} MB)` : "";
        const marks = result.firstFrameMs ? `, first interactive frame ${result.firstFrameMs.toFixed(0)} ms` : "";
        log(`  run ${i + 1}/${runs}${i === 0 ? " (warm-up, discarded)" : ""}: first window ${result.firstWindowMs.toFixed(0)} ms${marks}${mem}`);
        starts.push(result);
      } finally {
        watcher.stop();
        await stopAll(child, mine);
        await extra.cleanup?.();
      }
      // Let the previous run's processes go away.
      await sleep(1000);
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  return summarizeStarts(starts);
}

/** Medians over the starts after the warm-up; memory: the median of the memory starts. */
export function summarizeStarts(starts) {
  const counted = starts.slice(1);
  const withMemory = starts.filter((s) => s.memory);
  const byOwn = [...withMemory].sort((a, b) => a.memory.ownMB - b.memory.ownMB);
  const medianMemory = byOwn[Math.floor((byOwn.length - 1) / 2)]?.memory ?? null;
  const frames = counted.map((s) => s.firstFrameMs).filter((v) => typeof v === "number");
  return {
    first_window: Math.round(median(counted.map((s) => s.firstWindowMs))),
    ...(frames.length > 0 ? { cold_start: Math.round(median(frames)) } : {}),
    idle_memory: medianMemory ? medianMemory.ownMB : undefined,
    idle_memory_rss: medianMemory ? medianMemory.rssMB : undefined,
    samples: {
      firstWindowMs: counted.map((s) => Math.round(s.firstWindowMs)),
      ...(frames.length > 0 ? { firstFrameMs: frames.map(Math.round) } : {}),
      memoryRunsMB: withMemory.map((s) => s.memory.ownMB),
      memory: medianMemory,
    },
  };
}
