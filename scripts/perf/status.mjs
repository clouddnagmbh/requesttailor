// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: the working copy of a repository with 50 000 tracked files, 100 of them
// changed (`git_status_large`, perf/README.md).
//
// A synthetic repository is made in a temporary folder (nested folders, 25 small files each,
// committed with the Git command line), then 100 files change: 40 modified, 30 new, 30
// deleted. `crates/rt-git/examples/perf_status.rs` (release build) times
// `Repo::working_copy` warm and prints the median of 9 runs; it also checks that the status
// lists exactly those 100 files. The folder is deleted afterwards.

import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const FILES = 50_000;
export const MODIFIED = 40;
export const NEW = 30;
export const DELETED = 30;

const GIT_ENV = {
  GIT_CONFIG_NOSYSTEM: "1",
  GIT_CONFIG_GLOBAL: "/dev/null",
  GIT_AUTHOR_NAME: "Perf",
  GIT_AUTHOR_EMAIL: "perf@example.com",
  GIT_COMMITTER_NAME: "Perf",
  GIT_COMMITTER_EMAIL: "perf@example.com",
};

/** The folder of the `i`-th tracked file: 10 x 10 x 20 folders of 25 files. */
export function folderOf(i) {
  const leaf = Math.floor(i / 25);
  return join(`a${Math.floor(leaf / 200)}`, `b${Math.floor(leaf / 20) % 10}`, `c${leaf % 20}`);
}

export function fileOf(i) {
  return join(folderOf(i), `file-${i}.yml`);
}

function git(cwd, ...args) {
  return execFileSync("git", args, { cwd, env: { ...process.env, ...GIT_ENV }, stdio: ["ignore", "pipe", "inherit"] }).toString();
}

/** Makes the repository in `dir`; returns how many files differ from `HEAD` afterwards. */
export function makeRepository(dir) {
  git(dir, "init", "-q", "-b", "main");
  for (let i = 0; i < FILES; i++) {
    mkdirSync(join(dir, folderOf(i)), { recursive: true });
    writeFileSync(join(dir, fileOf(i)), `request: ${i}\nurl: https://example.com/${i}\n`);
  }
  git(dir, "add", "-A");
  git(dir, "commit", "-q", "-m", "synthetic");
  // Let the index age past the second its files were written in (Git's "racy" rule), then
  // refresh it the way any `git status` does.
  execFileSync("sleep", ["2"]);
  git(dir, "status", "--porcelain");
  for (let i = 0; i < MODIFIED; i++) writeFileSync(join(dir, fileOf(i * 7)), `request: changed ${i}\n`);
  for (let i = 0; i < DELETED; i++) rmSync(join(dir, fileOf(FILES - 1 - i * 3)));
  mkdirSync(join(dir, "fresh", "deep"), { recursive: true });
  for (let i = 0; i < NEW; i++) writeFileSync(join(dir, "fresh", "deep", `new-${i}.yml`), `request: new ${i}\n`);
  return MODIFIED + DELETED + NEW;
}

/** @returns {Promise<{ results: Record<string, number>, details: Record<string, unknown> }>} */
export async function measureStatus({ root }) {
  const dir = mkdtempSync(join(tmpdir(), "rt-perf-status-"));
  try {
    const changed = makeRepository(dir);
    const args = ["run", "--release", "--locked", "-q", "-p", "rt-git", "--example", "perf_status", "--", dir, "9", String(changed)];
    console.log(`$ cargo ${args.join(" ")}`);
    const out = execFileSync("cargo", args, { cwd: root, stdio: ["ignore", "pipe", "inherit"], maxBuffer: 1 << 20 });
    const line = String(out ?? "").trim().split("\n").filter(Boolean).pop() ?? "";
    const r = JSON.parse(line);
    return {
      results: { git_status_large: Math.round(r.median_ms * 10) / 10 },
      details: { files: FILES, changed: r.files, runs_ms: r.runs_ms.map((v) => Math.round(v * 10) / 10) },
    };
  } finally {
    rmSync(dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 300 });
  }
}
