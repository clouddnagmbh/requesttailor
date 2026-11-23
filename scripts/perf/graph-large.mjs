// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: the commit graph of a repository with 100 000 commits (`git_graph_large`,
// perf/README.md): the first page natively plus the first rows in the view, median of 5.
//
// - Native part: a synthetic repository (a main line, feature branches that fork and merge back,
//   octopus merges, a second root; `git fast-import` from a stream made here, in a temporary
//   folder, deleted afterwards) and `cargo run --release -p rt-git --example perf_graph`: open the
//   repository and read the first page of 2000 rows, median of 5 after one warm-up.
// - View part: apps/web/e2e/perf/graph-large.measure.ts against the mock backend with the same
//   kind of history (`?graph=100000`): from opening "Git: History" until its first rows are on
//   screen, median of 5.
//
// `git_graph_large` is the sum of both medians.

import { spawn, spawnSync } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

export const COMMITS = 100_000;

/** A small seeded PRNG (mulberry32). */
function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const PEOPLE = ["Ada Lovelace", "Grace Hopper", "Linus Torvalds", "Margaret Hamilton", "Ken Thompson"];

/**
 * The `git fast-import` stream of a history of `count` commits: a main line, up to six feature
 * branches that fork from it and merge back (some as octopus merges), a second root, a few
 * hundred files of changing content. Yields strings.
 */
export function* fastImport(count, seed = 7) {
  const rnd = random(seed);
  const pick = (list) => list[Math.floor(rnd() * list.length)];
  const start = 1_600_000_000;
  let n = 0;
  const commit = (ref, parents, message) => {
    n++;
    const who = pick(PEOPLE);
    const time = start + n * 60;
    const body = `change ${n} ${(rnd() * 1e9) | 0}\n`;
    const dir = `dir${n % 40}`;
    let out = `commit ${ref}\nmark :${n}\nauthor ${who} <${who.split(" ")[0].toLowerCase()}@example.com> ${time} +0000\n`;
    out += `committer ${who} <${who.split(" ")[0].toLowerCase()}@example.com> ${time} +0000\ndata ${Buffer.byteLength(message)}\n${message}\n`;
    if (parents[0] !== undefined) out += `from :${parents[0]}\n`;
    for (const p of parents.slice(1)) out += `merge :${p}\n`;
    out += `M 100644 inline ${dir}/file${n % 25}.yml\ndata ${Buffer.byteLength(body)}\n${body}\n`;
    return { mark: n, text: out };
  };
  let main = null;
  let docs = null;
  const features = [];
  const chunk = [];
  const emit = (c) => {
    chunk.push(c.text);
    return c.mark;
  };
  main = emit(commit("refs/heads/main", [], "Start"));
  const docsAt = Math.floor(count * 0.3);
  while (n < count) {
    if (docs === null && n >= docsAt) {
      docs = emit(commit("refs/heads/docs", [], "Start the docs"));
    } else if (docs !== null && rnd() < 0.01) {
      docs = emit(commit("refs/heads/docs", [docs], `Docs ${n}`));
    } else {
      const r = rnd();
      if (r < 0.06 && features.length < 6) {
        const f = { name: `refs/heads/feature/${n}`, tip: main };
        f.tip = emit(commit(f.name, [f.tip], `Start feature ${n}`));
        features.push(f);
      } else if (r < 0.32 && features.length > 0) {
        const f = pick(features);
        f.tip = emit(commit(f.name, [f.tip], `Work on ${f.name} ${n}`));
      } else if (r < 0.42 && features.length > 0) {
        const three = features.length >= 3 && rnd() < 0.05;
        const taken = features.splice(0, three ? 3 : 1);
        main = emit(commit("refs/heads/main", [main, ...taken.map((f) => f.tip)], `Merge ${taken.map((f) => f.name).join(", ")}`));
        // A few merged branches stay around as refs (the others are deleted after the merge).
        for (const f of taken) {
          if (n % 25 === 0) chunk.push(`reset ${f.name}\nfrom :${f.tip}\n\n`);
          else chunk.push(`reset ${f.name}\n\n`);
        }
      } else {
        main = emit(commit("refs/heads/main", [main], `Change ${n}`));
      }
    }
    if (chunk.length >= 500) yield chunk.splice(0).join("");
  }
  for (const f of features) chunk.push(`reset ${f.name}\nfrom :${f.tip}\n\n`);
  chunk.push("done\n");
  yield chunk.splice(0).join("");
}

/** Makes the repository in `dir` with `git fast-import`; resolves when it is done. */
export function makeRepository(dir, count = COMMITS) {
  const init = spawnSync("git", ["init", "--quiet", "--initial-branch=main", dir], { stdio: "inherit" });
  if (init.status !== 0) throw new Error("git init failed");
  return new Promise((resolve, reject) => {
    const proc = spawn("git", ["-C", dir, "fast-import", "--quiet", "--done"], { stdio: ["pipe", "inherit", "inherit"] });
    proc.on("error", reject);
    proc.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`git fast-import exited with ${code}`))));
    const gen = fastImport(count);
    const pump = () => {
      for (;;) {
        const { value, done } = gen.next();
        if (done) return proc.stdin.end();
        if (!proc.stdin.write(value)) return proc.stdin.once("drain", pump);
      }
    };
    pump();
  });
}


/** Returns `{ results: { git_graph_large }, details }`. */
export async function measureGraphLarge(ctx) {
  const dir = mkdtempSync(join(tmpdir(), "rt-perf-graph-large-"));
  const out = mkdtempSync(join(tmpdir(), "rt-perf-graph-large-out-"));
  try {
    const repo = join(dir, "repo");
    const made = Date.now();
    await makeRepository(repo);
    const fixtureMs = Date.now() - made;
    const run = spawnSync("cargo", ["run", "--release", "--locked", "-q", "-p", "rt-git", "--example", "perf_graph", "--", repo, "5"], {
      cwd: ctx.root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "inherit"],
    });
    if (run.status !== 0) throw new Error("perf_graph failed");
    const native = JSON.parse(run.stdout.trim().split("\n").pop());
    if (native.rows < 2000 || !native.more) throw new Error(`the first page has ${native.rows} rows, more: ${native.more}`);

    ctx.run("npx", ["playwright", "test", "--config", "e2e/perf/graph-large.perf.config.ts"], {
      cwd: ctx.web,
      env: { ...process.env, RT_PERF_OUT: out },
    });
    const view = ctx.readJson(join(out, "graph-large.json"));
    const total = Math.round((native.median_ms + view.first_rows_ms) * 100) / 100;
    return {
      results: { git_graph_large: total },
      details: { commits: COMMITS, native_ms: native.median_ms, native_runs_ms: native.runs_ms, view_ms: view.first_rows_ms, view_runs_ms: view.runs, fixture_ms: fixtureMs },
    };
  } finally {
    rmSync(dir, { recursive: true, force: true });
    rmSync(out, { recursive: true, force: true });
  }
}

