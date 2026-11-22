// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Memory of the desktop app and its webview processes, with OS tools only (perf/README.md):
//
// | OS      | processes counted                                   | `own` (the gate)       | `rss` (reported)  |
// | ------- | --------------------------------------------------- | ---------------------- | ----------------- |
// | macOS   | the app + WebKit XPC services started for it        | phys_footprint (`footprint`) | `ps` RSS    |
// | Linux   | the app + all descendants (WebKitWebProcess, ...)   | PSS (smaps_rollup)     | VmRSS             |
// | Windows | the app + all descendants (msedgewebview2.exe)      | private working set    | working set       |
//
// On macOS the WebKit processes (com.apple.WebKit.WebContent, .Networking, .GPU) are XPC
// services whose parent is launchd, not the app; they are attributed to the app as the WebKit
// processes that did not exist before it started (`newWebKitPids`).

import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";

const MB = 1024 * 1024;

// ---- parsing (pure, tested in memory.test.mjs) ----

/** `ps -axo pid=,ppid=,rss=,comm=` output → [{ pid, ppid, rssBytes, name }]. */
export function parsePs(text) {
  const out = [];
  for (const line of text.split("\n")) {
    const m = line.trim().match(/^(\d+)\s+(\d+)\s+(\d+)\s+(.+)$/);
    if (m) out.push({ pid: Number(m[1]), ppid: Number(m[2]), rssBytes: Number(m[3]) * 1024, name: m[4].trim() });
  }
  return out;
}

/** `footprint --noCategories -f bytes <pids>` output → Map(pid → bytes). */
export function parseFootprint(text) {
  const out = new Map();
  for (const m of text.matchAll(/\[(\d+)\]:.*?Footprint:\s*(\d+)\s*B/g)) out.set(Number(m[1]), Number(m[2]));
  return out;
}

/** /proc/<pid>/stat → ppid. The name (2nd field) may contain spaces and parentheses. */
export function parseProcStatPpid(text) {
  const rest = text.slice(text.lastIndexOf(")") + 2).split(" ");
  return Number(rest[1]);
}

/** A `Key:   1234 kB` field of /proc/<pid>/status or smaps_rollup, in bytes (or null). */
export function procField(text, key) {
  const m = text.match(new RegExp(`^${key}:\\s+(\\d+)\\s*kB`, "m"));
  return m ? Number(m[1]) * 1024 : null;
}

/** Every pid in `procs` below `root` (children, grandchildren, ...), including `root`. */
export function descendants(procs, root) {
  const children = new Map();
  for (const p of procs) {
    if (!children.has(p.ppid)) children.set(p.ppid, []);
    children.get(p.ppid).push(p.pid);
  }
  const seen = new Set([root]);
  const stack = [root];
  while (stack.length > 0) {
    for (const c of children.get(stack.pop()) ?? []) {
      if (!seen.has(c)) {
        seen.add(c);
        stack.push(c);
      }
    }
  }
  return seen;
}

export const isWebKitXpc = (name) => /com\.apple\.WebKit\.(WebContent|Networking|GPU|Model)\b/.test(name);

/** WebKit XPC processes in `procs` that were not running before the app started. */
export function newWebKitPids(procs, before) {
  return procs.filter((p) => isWebKitXpc(p.name) && !before.has(p.pid)).map((p) => p.pid);
}

/** Sums a sample into { ownMB, rssMB, processes: [{ pid, name, ownMB, rssMB }] }. */
export function summarize(processes) {
  const round = (b) => Math.round((b / MB) * 10) / 10;
  const list = processes.map((p) => ({ pid: p.pid, name: shortName(p.name), ownMB: round(p.own), rssMB: round(p.rss) }));
  return {
    ownMB: round(processes.reduce((s, p) => s + p.own, 0)),
    rssMB: round(processes.reduce((s, p) => s + p.rss, 0)),
    processes: list,
  };
}

function shortName(name) {
  return name.split(/[\\/]/).pop();
}

// ---- OS adapters ----

function sh(cmd, args) {
  return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * MB });
}

/** What to remember before the app starts (macOS: the WebKit processes already running). */
export function baseline() {
  if (process.platform !== "darwin") return new Set();
  return new Set(parsePs(sh("ps", ["-axo", "pid=,ppid=,rss=,comm="])).filter((p) => isWebKitXpc(p.name)).map((p) => p.pid));
}

/** Memory of the app `pid` and its webview processes now. */
export function sample(pid, before) {
  switch (process.platform) {
    case "darwin":
      return sampleMac(pid, before);
    case "linux":
      return sampleLinux(pid);
    case "win32":
      return sampleWindows(pid);
    default:
      throw new Error(`memory measurement is not implemented for ${process.platform}`);
  }
}

function sampleMac(pid, before) {
  const procs = parsePs(sh("ps", ["-axo", "pid=,ppid=,rss=,comm="]));
  const tree = descendants(procs, pid);
  const pids = [...new Set([...tree, ...newWebKitPids(procs, before)])];
  const footprints = parseFootprint(sh("footprint", ["--noCategories", "-f", "bytes", ...pids.map(String)]));
  const byPid = new Map(procs.map((p) => [p.pid, p]));
  const list = pids
    .filter((p) => byPid.has(p))
    .map((p) => ({ pid: p, name: byPid.get(p).name, rss: byPid.get(p).rssBytes, own: footprints.get(p) ?? byPid.get(p).rssBytes }));
  const webContent = list.filter((p) => /WebContent/.test(p.name)).length;
  return { ...summarize(list), warning: webContent === 1 ? null : `expected one WebKit WebContent process, found ${webContent}` };
}

function sampleLinux(pid) {
  const procs = [];
  for (const d of readdirSync("/proc")) {
    if (!/^\d+$/.test(d)) continue;
    try {
      procs.push({ pid: Number(d), ppid: parseProcStatPpid(readFileSync(`/proc/${d}/stat`, "utf8")) });
    } catch {
      // The process ended while listing.
    }
  }
  const list = [];
  for (const p of descendants(procs, pid)) {
    try {
      const status = readFileSync(`/proc/${p}/status`, "utf8");
      const rss = procField(status, "VmRSS") ?? 0;
      let own = rss;
      try {
        own = procField(readFileSync(`/proc/${p}/smaps_rollup`, "utf8"), "Pss") ?? rss;
      } catch {
        // Kernels without smaps_rollup: RSS.
      }
      const name = (status.match(/^Name:\s*(.+)$/m) ?? [])[1] ?? String(p);
      list.push({ pid: p, name, rss, own });
    } catch {
      // Ended.
    }
  }
  return { ...summarize(list), warning: list.some((p) => /WebKitWebProcess/.test(p.name)) ? null : "no WebKitWebProcess found" };
}

function sampleWindows(pid) {
  const script = [
    "$p = Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,Name,WorkingSetSize",
    "$f = Get-CimInstance Win32_PerfRawData_PerfProc_Process | Select-Object IDProcess,WorkingSetPrivate",
    "@{ procs = $p; perf = $f } | ConvertTo-Json -Depth 3 -Compress",
  ].join("; ");
  const data = JSON.parse(sh("powershell", ["-NoProfile", "-NonInteractive", "-Command", script]));
  const procs = data.procs.map((p) => ({ pid: p.ProcessId, ppid: p.ParentProcessId, name: p.Name, rss: Number(p.WorkingSetSize) }));
  const priv = new Map(data.perf.map((f) => [f.IDProcess, Number(f.WorkingSetPrivate)]));
  const tree = descendants(procs, pid);
  const list = procs.filter((p) => tree.has(p.pid)).map((p) => ({ ...p, own: priv.get(p.pid) ?? p.rss }));
  return { ...summarize(list), warning: list.some((p) => /msedgewebview2/i.test(p.name)) ? null : "no msedgewebview2 process found" };
}
