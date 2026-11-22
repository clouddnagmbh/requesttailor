// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tests of the measurement tools' pure parts (parsing, attribution, statistics, merging).
// Run: node --test scripts/perf/*.test.mjs

import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { isSettled, median, parseMarks, recentFile } from "./desktop.mjs";
import {
  descendants,
  newWebKitPids,
  parseFootprint,
  parseProcStatPpid,
  parsePs,
  procField,
  summarize,
} from "./memory.mjs";
import { mergeWorst } from "./merge.mjs";
import { defaultBinary, parseArgs, worstP95 } from "./run.mjs";

test("marks: first value per name, junk ignored", () => {
  assert.deepEqual(parseMarks("core-ready 221.4\nfirst-frame 431.0\nfirst-frame 999\nnoise\n"), {
    "core-ready": 221.4,
    "first-frame": 431,
  });
});

test("median and idle detection", () => {
  assert.equal(median([3, 1, 2]), 2);
  assert.equal(median([4, 1, 2, 3]), 2.5);
  assert.ok(Number.isNaN(median([])));
  assert.equal(isSettled([100, 120]), false);
  assert.equal(isSettled([150, 120, 100.5, 101, 100]), true);
  assert.equal(isSettled([100, 110, 120]), false);
});

test("the recent list for the measured start names the canonical collection folder", () => {
  const dir = mkdtempSync(join(tmpdir(), "rt-perf-recent-"));
  try {
    const f = JSON.parse(recentFile(dir));
    assert.equal(f.version, 1);
    assert.equal(f.collections.length, 1);
    assert.ok(!f.collections[0].id.startsWith("\\\\?\\"));
    assert.equal(typeof f.collections[0].openedAt, "number");
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

const PS = `
  1     0  1000 /sbin/launchd
 500     1 104200 /tmp/target/release/rt-desktop
 501     1 32600 /System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.GPU.xpc/Contents/MacOS/com.apple.WebKit.GPU
 502     1 16000 /System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.Networking.xpc/Contents/MacOS/com.apple.WebKit.Networking
 503     1 60600 /System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent
 400     1 88000 /System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent
 600   500  2000 /tmp/child helper
`;

test("macOS: ps parsing and WebKit attribution by baseline", () => {
  const procs = parsePs(PS);
  assert.equal(procs.length, 7);
  assert.deepEqual(procs[1], { pid: 500, ppid: 1, rssBytes: 104200 * 1024, name: "/tmp/target/release/rt-desktop" });
  // 400 ran before the app started (another app's web content): not counted.
  assert.deepEqual(newWebKitPids(procs, new Set([400])).sort(), [501, 502, 503]);
  assert.deepEqual([...descendants(procs, 500)].sort(), [500, 600]);
});

test("macOS: footprint output", () => {
  const text = `
======================================================================
com.apple.WebKit.Networking [19816]: 64-bit    Footprint: 9635200 B (16384 bytes per page)
======================================================================
rt-desktop [19517]: 64-bit    Footprint: 31337 B (16384 bytes per page)
Summary Footprint: 119201472 B
`;
  assert.deepEqual([...parseFootprint(text)], [
    [19816, 9635200],
    [19517, 31337],
  ]);
});

test("Linux: /proc parsing", () => {
  assert.equal(parseProcStatPpid("1234 (Web Content (x)) S 42 1234 1234 0 -1"), 42);
  const status = "Name:\tWebKitWebProcess\nVmRSS:\t   81234 kB\nPss:  5 kB\n";
  assert.equal(procField(status, "VmRSS"), 81234 * 1024);
  assert.equal(procField(status, "Pss"), 5 * 1024);
  assert.equal(procField(status, "Missing"), null);
});

test("summaries add up in MB", () => {
  const MB = 1024 * 1024;
  const s = summarize([
    { pid: 1, name: "/x/rt-desktop", own: 30 * MB, rss: 100 * MB },
    { pid: 2, name: "C:\\w\\msedgewebview2.exe", own: 70.04 * MB, rss: 90 * MB },
  ]);
  assert.equal(s.ownMB, 100);
  assert.equal(s.rssMB, 190);
  assert.deepEqual(
    s.processes.map((p) => p.name),
    ["rt-desktop", "msedgewebview2.exe"],
  );
});

test("merge keeps the worst value per metric", () => {
  assert.deepEqual(mergeWorst([{ cold_start: 300, idle_memory: 120 }, { cold_start: 450 }, { large_response_frame: 16.8 }]), {
    cold_start: 450,
    idle_memory: 120,
    large_response_frame: 16.8,
  });
  assert.throws(() => mergeWorst([{ cold_start: "x" }]), /not a number/);
});

test("run options", () => {
  const all = parseArgs([]);
  assert.deepEqual(all.only, ["desktop", "frames", "keystrokes"]);
  assert.equal(all.partial, undefined);
  const some = parseArgs(["--only", "desktop", "--runs", "5", "--memory-runs", "2", "--check"]);
  assert.deepEqual(some.only, ["desktop"]);
  assert.equal(some.partial, true);
  assert.equal(some.runs, 5);
  assert.equal(some.check, true);
  assert.throws(() => parseArgs(["--only", "gpu"]), /unknown part/);
  assert.throws(() => parseArgs(["--runs", "1"]), /at least 2/);
  assert.throws(() => parseArgs(["--runs", "3", "--memory-runs", "3"]), /less than --runs/);
  assert.match(defaultBinary({ CARGO_TARGET_DIR: "/t" }, "win32"), /rt-desktop\.exe$/);
  assert.match(defaultBinary({}, "linux"), /target[\\/]release[\\/]rt-desktop$/);
});

test("the frame budget sees the worst variant, and only when all were measured", () => {
  assert.equal(worstP95([{ p95: 16.8 }, { p95: 18.2 }]), 18.2);
  assert.equal(worstP95([{ p95: 16.8 }, {}]), undefined);
  assert.equal(worstP95([]), undefined);
});
