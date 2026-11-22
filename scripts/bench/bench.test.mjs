// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tests of the benchmark's pure parts (arguments, app discovery, process attribution,
// statistics, tables) and that the website shows the latest committed result.
// Run: node --test scripts/bench/*.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { discover, electronIsolation, executableOf, expandPath } from "./apps.mjs";
import { ownProcesses, summarizeStarts } from "./launch.mjs";
import { END, NO_RESULT, START, budgetMaxima, cell, latestResult, markdown, replaceBlock, rows, websiteBlock } from "./report.mjs";
import { osSlug, parseArgs } from "./run.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..", "..");
const BUDGETS = { cold_start: 500, idle_memory: 150, large_response_frame: 20, send_keystrokes: 2 };

function sampleResult() {
  return {
    version: 1,
    date: "2026-11-22T12:00:00.000Z",
    machine: { model: "MacBookPro18,2", arch: "arm64", cpu: "Apple M1 Max", cpus: 10, memoryGB: 32, osName: "macOS", osVersion: "26.2 (25C56)" },
    node: "v26.5.0",
    loadAverage: [1.2, 1.5, 1.7],
    method: { runs: 10, memoryRuns: 3 },
    apps: [
      { id: "requesttailor", name: "RequestTailor", installed: true, version: "0.1.0", commit: "abc1234", notApplicable: {}, results: { first_window: 120, cold_start: 310, idle_memory: 96.4, large_response_frame: 16.8, send_keystrokes: 1 }, notes: ["internal mark"] },
      { id: "postman", name: "Postman", installed: true, version: "12.32.4", notApplicable: { cold_start: true, large_response_frame: true, send_keystrokes: true }, results: { first_window: 2100, idle_memory: 612.5 }, notes: ["fresh profile"] },
      { id: "insomnia", name: "Insomnia", installed: false, searched: ["/Applications/Insomnia.app"] },
      { id: "bruno", name: "Bruno", installed: true, version: "2.0.0", error: "no window within 60000 ms" },
    ],
  };
}

test("arguments", () => {
  const d = parseArgs([]);
  assert.deepEqual(d.apps, ["requesttailor", "postman", "insomnia", "bruno", "hoppscotch"]);
  assert.equal(d.runs, 10);
  assert.equal(d.memoryRuns, 3);
  assert.equal(d.website, true);
  const o = parseArgs(["--apps", "requesttailor,postman", "--runs", "4", "--memory-runs", "2", "--app", "postman=/x/Postman.app", "--no-website"]);
  assert.deepEqual(o.apps, ["requesttailor", "postman"]);
  assert.equal(o.overrides.postman, "/x/Postman.app");
  assert.equal(o.website, false);
  assert.throws(() => parseArgs(["--apps", "curl"]), /unknown app/);
  assert.throws(() => parseArgs(["--runs", "1"]), /at least 2/);
  assert.throws(() => parseArgs(["--memory-runs", "10"]), /less than --runs/);
  assert.throws(() => parseArgs(["--app", "requesttailor=/x"]), /--app needs/);
  assert.equal(osSlug("darwin"), "macos");
  assert.equal(osSlug("win32"), "windows");
  assert.equal(osSlug("linux"), "linux");
});

test("discovery: installed apps, not installed ones, overrides", () => {
  const present = new Set(["/Applications/Postman.app", "/opt/bruno"]);
  const apps = discover({ os: "darwin", exists: (p) => present.has(p), overrides: { bruno: "/opt/bruno" } });
  const byId = Object.fromEntries(apps.map((a) => [a.id, a]));
  assert.equal(byId.postman.installed, true);
  assert.equal(byId.postman.path, "/Applications/Postman.app");
  assert.equal(byId.insomnia.installed, false);
  assert.ok(byId.insomnia.searched.includes("/Applications/Insomnia.app"));
  assert.equal(byId.bruno.path, "/opt/bruno");
  assert.equal(byId.hoppscotch.engine, "webkit");
  assert.equal(expandPath("~/Applications/X.app", {}, "/Users/me"), "/Users/me/Applications/X.app");
  assert.equal(expandPath("%LOCALAPPDATA%\\Postman\\Postman.exe", { LOCALAPPDATA: "C:\\L" }), "C:\\L\\Postman\\Postman.exe");
  assert.equal(executableOf("/usr/bin/postman", "linux"), "/usr/bin/postman");
  assert.deepEqual(electronIsolation("/tmp/p"), ["--user-data-dir=/tmp/p", "--proxy-server=127.0.0.1:9"]);
});

test("only processes started for the run are attributed and stopped", () => {
  const procs = [
    { pid: 10, ppid: 1, name: "/Applications/Postman.app/Contents/MacOS/Postman" },
    { pid: 11, ppid: 10, name: "/Applications/Postman.app/Contents/Frameworks/Postman Helper (GPU).app/Contents/MacOS/Postman Helper (GPU)" },
    // The crashpad handler is re-parented to launchd.
    { pid: 12, ppid: 1, name: "/Applications/Postman.app/Contents/Frameworks/Electron Framework.framework/Helpers/chrome_crashpad_handler" },
    // Started before the run: not ours.
    { pid: 5, ppid: 1, name: "/Applications/Postman.app/Contents/MacOS/Postman" },
    { pid: 20, ppid: 1, name: "/usr/bin/other" },
  ];
  assert.deepEqual(ownProcesses(procs, 10, new Set([5, 20]), "/Applications/Postman.app").sort(), [10, 11, 12]);
  assert.deepEqual(ownProcesses(procs, 10, new Set([5, 20]), null).sort(), [10, 11]);
});

test("medians after the warm-up start", () => {
  const starts = [
    { firstWindowMs: 5000 },
    { firstWindowMs: 300, firstFrameMs: 410 },
    { firstWindowMs: 100, firstFrameMs: 390 },
    { firstWindowMs: 200, firstFrameMs: 400, memory: { ownMB: 90, rssMB: 300 } },
    { firstWindowMs: 250, firstFrameMs: 420, memory: { ownMB: 80, rssMB: 290 } },
    { firstWindowMs: 260, firstFrameMs: 430, memory: { ownMB: 100, rssMB: 310 } },
  ];
  const s = summarizeStarts(starts);
  assert.equal(s.first_window, 250);
  assert.equal(s.cold_start, 410);
  assert.equal(s.idle_memory, 90);
  assert.equal(s.idle_memory_rss, 300);
  assert.equal(summarizeStarts([{ firstWindowMs: 1 }, { firstWindowMs: 2 }]).cold_start, undefined);
});

test("tables: budget row, values, n/a, not installed, failed", () => {
  const r = sampleResult();
  const table = rows(r, BUDGETS);
  assert.deepEqual(table[0].cells, ["", "≤ 500 ms", "≤ 150 MB", "≤ 20 ms", "≤ 2"]);
  assert.deepEqual(table[1].cells, ["120 ms", "310 ms", "96.4 MB", "16.8 ms", "1"]);
  assert.equal(table[1].version, "0.1.0 (abc1234)");
  assert.deepEqual(table[2].cells, ["2100 ms", "n/a", "612.5 MB", "n/a", "n/a"]);
  assert.deepEqual(table[3].cells, Array(5).fill("not installed"));
  assert.deepEqual(table[4].cells, Array(5).fill("failed"));
  assert.equal(cell({ installed: true, results: {} }, { key: "idle_memory", unit: "MB" }), "not measured");

  const md = markdown(r, BUDGETS);
  assert.match(md, /\| Budget \(RequestTailor\) \| perf\/budgets\.toml \|  \| ≤ 500 ms \|/);
  assert.match(md, /- Bruno: no window within 60000 ms/);
  assert.match(md, /Load average at start: 1.2, 1.5, 1.7/);

  const html = websiteBlock(r, BUDGETS);
  assert.ok(html.startsWith(START) && html.endsWith(END));
  for (const [metric, max] of Object.entries(BUDGETS)) assert.ok(html.includes(`<span data-budget="${metric}">${max}</span>`), metric);
  assert.ok(html.includes("<th scope=\"row\">Postman</th><td>12.32.4</td><td>2100 ms</td>"));
  assert.ok(!html.includes("<script"));
});

test("the generated block replaces only itself", () => {
  const page = `<p>before</p>\n${START}\nold\n${END}\n<p>after</p>\n`;
  assert.equal(replaceBlock(page, `${START}\nnew\n${END}`), `<p>before</p>\n${START}\nnew\n${END}\n<p>after</p>\n`);
  assert.throws(() => replaceBlock("<p>no block</p>", "x"), /no/);
});

test("the newest result file wins", () => {
  const dir = mkdtempSync(join(tmpdir(), "rt-bench-test-"));
  try {
    for (const f of ["2026-11-01-macos.json", "2026-11-22-macos.json", "notes.json", "2026-11-22-macos.md"]) writeFileSync(join(dir, f), "{}");
    assert.equal(latestResult(dir), join(dir, "2026-11-22-macos.json"));
    assert.equal(latestResult(join(dir, "missing")), null);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("the website shows the latest committed result with the budgets of perf/budgets.toml", () => {
  const page = readFileSync(join(ROOT, "website", "benchmark", "index.html"), "utf8");
  const block = page.slice(page.indexOf(START), page.indexOf(END) + END.length);
  const file = latestResult();
  if (file === null) {
    // Before the first measurement on a quiet machine is committed.
    assert.equal(block, NO_RESULT);
    return;
  }
  const budgets = budgetMaxima(readFileSync(join(ROOT, "perf", "budgets.toml"), "utf8"));
  const latest = JSON.parse(readFileSync(file, "utf8"));
  assert.equal(block, websiteBlock(latest, budgets), "run: node scripts/bench/report.mjs --website website/benchmark/index.html");
});
