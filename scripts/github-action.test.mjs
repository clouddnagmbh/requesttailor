// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tests of the GitHub Action (github-action/): the job summary and outputs from the JSON
// report of `rt run` (the golden report of crates/rt-cli/tests/run.rs), the download with
// its checksum check against a release laid out in a folder, and the arguments run.sh gives
// rt (a fake rt records them). No network, no dependencies.

import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { cell, markdown, outputs, main } from "../github-action/summary.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const action = join(root, "github-action");
const golden = JSON.parse(readFileSync(join(root, "crates/rt-cli/tests/golden/run.json"), "utf8"));
const unix = process.platform !== "win32";

function temp() {
  return mkdtempSync(join(tmpdir(), "rt-action-test-"));
}

test("outputs from the summary", () => {
  assert.deepEqual(outputs(golden), {
    passed: 3,
    failed: 3,
    skipped: 2,
    "tests-passed": 3,
    "tests-failed": 1,
    "assertions-passed": 2,
    "assertions-failed": 0,
  });
});

test("job summary: a row per step, failures listed, text escaped", () => {
  const md = markdown(golden, 4);
  assert.match(md, /^### ❌ rt run: 3 of 6 requests passed/);
  const rows = md.split("\n").filter((l) => /^\| (✅|❌|⏭️) \|/.test(l));
  assert.equal(rows.length, 6);
  // Two iterations: an iteration column.
  assert.match(md, /^\|  \| # \| Request \| Status \| Time \| Tests \| Assertions \|$/m);
  assert.match(rows[0], /^\| ✅ \| 1 \| GET Get user <sub>users\/get\.yml<\/sub> \| 200 \| 12 ms \| 2\/2 \| 1\/1 \|$/);
  assert.match(rows[1], /transport: connect: connection refused/);
  assert.match(rows[2], /^\| ⏭️ /);
  // The failed test, with HTML and Markdown escaped; the control character is gone.
  assert.match(md, /- \*\*Get user \(iteration 2\)\*\*: name is &lt;"Ann" &amp; 'Bob'&gt;: expected 'Bo b' to equal 'Bob' at line 2$/m);
  assert.ok(!md.includes("\u001b"));
  assert.ok(!md.includes("<\"Ann\""));
});

test("a passing run", () => {
  const ok = { version: 1, summary: { requests: 1, passed: 1, failed: 0, skipped: 0, duration_ms: 5 }, steps: [] };
  assert.match(markdown(ok, 0), /^### ✅ rt run: 1 of 1 requests passed/);
  assert.match(markdown(ok, 2), /exit code 2/);
});

test("cells cannot break the table or add markup", () => {
  assert.equal(cell("a|b\nc"), "a\\|b c");
  assert.equal(cell("<img src=x>"), "&lt;img src=x&gt;");
  assert.equal(cell("*bold* _x_ `c` [l](u) \\"), "\\*bold\\* \\_x\\_ \\`c\\` \\[l\\](u) \\\\");
});

test("main writes the outputs and the summary", () => {
  const dir = temp();
  try {
    const report = join(dir, "r.json");
    writeFileSync(report, JSON.stringify(golden));
    main([report, "4", join(dir, "out"), join(dir, "summary.md")]);
    assert.match(readFileSync(join(dir, "out"), "utf8"), /^passed=3\nfailed=3\n/);
    assert.match(readFileSync(join(dir, "summary.md"), "utf8"), /rt run: 3 of 6/);
    writeFileSync(report, JSON.stringify({ ...golden, version: 2 }));
    assert.throws(() => main([report, "0", join(dir, "out"), "/dev/null"]), /version 2/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ------------------------------------------------------------------------------- install.sh

function sha256(file) {
  return createHash("sha256").update(readFileSync(file)).digest("hex");
}

/** A release v9.9.9 in a folder, with an rt for this machine; returns the releases URL. */
function fakeRelease(dir, { corrupt = false, unlisted = false } = {}) {
  const v = "9.9.9";
  const darwin = process.platform === "darwin";
  const arch = { x64: "x86_64", arm64: "aarch64" }[process.arch];
  const folder = darwin ? `rt_${v}_macos_universal` : `rt_${v}_linux_${arch}`;
  const asset = darwin ? `${folder}.zip` : `${folder}.tar.gz`;
  const stage = join(dir, "stage");
  mkdirSync(join(stage, folder), { recursive: true });
  writeFileSync(join(stage, folder, "rt"), `#!/bin/sh\necho "rt ${v}"\n`);
  chmodSync(join(stage, folder, "rt"), 0o755);
  const download = join(dir, "releases", "download", `v${v}`);
  mkdirSync(download, { recursive: true });
  const pack = darwin
    ? spawnSync("zip", ["-qr", join(download, asset), folder], { cwd: stage })
    : spawnSync("tar", ["-czf", join(download, asset), folder], { cwd: stage });
  assert.equal(pack.status, 0, String(pack.stderr));
  const hash = corrupt ? "0".repeat(64) : sha256(join(download, asset));
  const other = `${"1".repeat(64)}  rt_${v}_other.tar.gz\n`;
  writeFileSync(join(download, "SHA256SUMS"), unlisted ? other : `${other}${hash}  ${asset}\n`);
  writeFileSync(join(dir, "releases", "latest"), `v${v}\n`);
  return `file://${join(dir, "releases")}`;
}

function install(releases, version, dir) {
  const out = join(dir, "github-output");
  writeFileSync(out, "");
  const r = spawnSync(join(action, "install.sh"), [], {
    env: { ...process.env, RT_ACTION_RELEASES: releases, INPUT_VERSION: version, GITHUB_OUTPUT: out, RUNNER_TEMP: dir },
    encoding: "utf8",
  });
  return { ...r, output: readFileSync(out, "utf8") };
}

test("install.sh downloads, checks and unpacks rt", { skip: !unix }, () => {
  const dir = temp();
  try {
    const releases = fakeRelease(dir);
    for (const version of ["latest", "9.9.9", "v9.9.9"]) {
      const r = install(releases, version, dir);
      assert.equal(r.status, 0, r.stderr);
      assert.match(r.stdout, /SHA-256 matches/);
      assert.match(r.stdout, /^rt 9\.9\.9$/m);
      const path = r.output.match(/^path=(.*)$/m)[1];
      assert.equal(spawnSync(path, ["--version"], { encoding: "utf8" }).stdout, "rt 9.9.9\n");
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("install.sh refuses a file that does not match SHA256SUMS", { skip: !unix }, () => {
  for (const [opts, message] of [
    [{ corrupt: true }, /SHA256SUMS says 0{64}\. Not using it/],
    [{ unlisted: true }, /is not listed in SHA256SUMS/],
  ]) {
    const dir = temp();
    try {
      const r = install(fakeRelease(dir, opts), "latest", dir);
      assert.equal(r.status, 1);
      assert.match(r.stderr, message);
      assert.equal(r.output, "", "no path output");
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  }
  const dir = temp();
  try {
    const r = install(fakeRelease(dir), "1.2.3", dir);
    assert.equal(r.status, 1);
    assert.match(r.stderr, /Does release v1\.2\.3 exist/);
    assert.match(install(fakeRelease(dir), "../x", dir).stderr, /Cannot tell the rt version/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

// ----------------------------------------------------------------------------------- run.sh

/** A fake rt: records its arguments (one per line), writes the golden report, exits `code`. */
function fakeRt(dir, code) {
  const rt = join(dir, "rt");
  writeFileSync(
    join(dir, "report.json"),
    JSON.stringify(golden),
  );
  writeFileSync(
    rt,
    `#!/usr/bin/env bash
printf '%s\\n' "$@" > "${join(dir, "args")}"
for a in "$@"; do
  case "$a" in json=*) cp "${join(dir, "report.json")}" "\${a#json=}" ;; junit=*) mkdir -p "$(dirname "\${a#junit=}")" && echo '<testsuites/>' > "\${a#junit=}" ;; esac
done
echo "::warning::injected from a response"
exit ${code}
`,
  );
  chmodSync(rt, 0o755);
  return rt;
}

function runAction(dir, env) {
  const out = join(dir, "github-output");
  const summary = join(dir, "summary.md");
  writeFileSync(out, "");
  writeFileSync(summary, "");
  const r = spawnSync(join(action, "run.sh"), [], {
    cwd: dir,
    env: { PATH: process.env.PATH, RUNNER_TEMP: dir, GITHUB_OUTPUT: out, GITHUB_STEP_SUMMARY: summary, ...env },
    encoding: "utf8",
  });
  return {
    ...r,
    output: readFileSync(out, "utf8"),
    summary: readFileSync(summary, "utf8"),
    args: readFileSync(join(dir, "args"), "utf8").split("\n").slice(0, -1),
  };
}

test("run.sh passes the inputs and secrets to rt run and sets the outputs", { skip: !unix }, () => {
  const dir = temp();
  try {
    const rt = fakeRt(dir, 4);
    const r = runAction(dir, {
      RT_BIN: rt,
      INPUT_COLLECTION: " api/users ",
      INPUT_ENVIRONMENT: "Staging",
      INPUT_VARS: "# comment\nbase=http://localhost:3000\n\n  id=7  \n",
      INPUT_DATA: "rows.csv",
      INPUT_ITERATIONS: "2",
      INPUT_JUNIT: "reports/junit.xml",
      INPUT_JSON: "",
      INPUT_NO_SCRIPTS: "true",
      INPUT_BAIL: "false",
      INPUT_ARGS: "--delay\n100\n",
      INPUT_SUMMARY: "true",
      RT_VAR_token: "s3cr3t value with spaces",
      RT_VARIANT: "not a variable",
    });
    assert.equal(r.status, 4, r.stderr);
    const report = r.args.find((a) => a.startsWith("json=")).slice(5);
    assert.deepEqual(r.args, [
      "run",
      "api/users",
      "--no-prompt",
      "--env",
      "Staging",
      "--data",
      "rows.csv",
      "--iterations",
      "2",
      "--no-scripts",
      "--var",
      "base=http://localhost:3000",
      "--var",
      "id=7",
      "--var",
      "token=s3cr3t value with spaces",
      "--reporter",
      "junit=reports/junit.xml",
      "--reporter",
      `json=${report}`,
      "--delay",
      "100",
    ]);
    assert.ok(report.startsWith(dir), report);
    // The secret is never printed by the action.
    assert.ok(!r.stdout.includes("s3cr3t") && !r.stderr.includes("s3cr3t"));
    // rt's own output cannot issue workflow commands.
    const stop = r.stdout.match(/^::stop-commands::(rt-[0-9a-f]{16})$/m);
    assert.ok(stop, r.stdout);
    const lines = r.stdout.split("\n");
    const injected = lines.indexOf("::warning::injected from a response");
    assert.ok(lines.indexOf(stop[0]) < injected && injected < lines.indexOf(`::${stop[1]}::`), r.stdout);
    assert.match(r.output, /^exit-code=4$/m);
    assert.match(r.output, /^junit=reports\/junit\.xml$/m);
    assert.match(r.output, new RegExp(`^report=${report.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")}$`, "m"));
    assert.match(r.output, /^passed=3$/m);
    assert.match(r.output, /^tests-failed=1$/m);
    assert.match(r.summary, /rt run: 3 of 6 requests passed/);
    assert.match(r.stdout, /::error::rt run: a request, test or assertion failed/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});

test("run.sh: defaults, no summary when turned off, bad vars", { skip: !unix }, () => {
  const dir = temp();
  try {
    const rt = fakeRt(dir, 0);
    const r = runAction(dir, { RT_BIN: rt, INPUT_JUNIT: "", INPUT_SUMMARY: "false", INPUT_BAIL: "true" });
    assert.equal(r.status, 0, r.stderr);
    assert.deepEqual(r.args.slice(0, 4), ["run", ".", "--no-prompt", "--bail"]);
    assert.ok(!r.args.some((a) => a.startsWith("junit=")));
    assert.match(r.output, /^junit=$/m);
    assert.equal(r.summary, "");
    const bad = spawnSync(join(action, "run.sh"), [], {
      env: { PATH: process.env.PATH, RT_BIN: rt, INPUT_VARS: "a=1\nnot a pair" },
      encoding: "utf8",
    });
    assert.equal(bad.status, 2);
    assert.match(bad.stderr, /vars, line 2: expected NAME=VALUE/);
    const missing = spawnSync(join(action, "run.sh"), [], { env: { PATH: process.env.PATH, RT_BIN: join(dir, "nope") }, encoding: "utf8" });
    assert.equal(missing.status, 2);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
