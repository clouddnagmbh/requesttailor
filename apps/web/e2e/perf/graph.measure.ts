// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Budgets `git_graph` and `git_graph_frame` (wave 14, strand F): a repository with 10 000
// commits (mock backend, `?graph=10000`: the synthetic history of `git/graph/synthetic.ts`, with
// merges, side branches, a second root and tags).
// - `git_graph`: from opening "Git: History" until its first rows are on screen, ms. Median of
//   five openings after one warm-up.
// - `git_graph_frame`: p95 frame time while scrolling the whole history, ms: a fast wheel-like
//   scroll (900 px per frame, through all pages, which load while scrolling), jumps to random
//   places, and PageDown on the list (keyboard), 240 frames each.
// Only measures; the gate is `node scripts/check-perf-budgets.mjs` over what
// `scripts/perf/run.mjs` collects.

import { expect, test, type Page } from "@playwright/test";

import { percentile, writePerfResult } from "./frames";

const MOD = "ControlOrMeta";
const COMMITS = 10_000;
const RUNS = 5;
const FRAMES = 240;

const round = (n: number) => Math.round(n * 100) / 100;

async function openPalette(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("Git: History");
  await expect(dialog.getByRole("option").filter({ hasText: "Git: History" }).first()).toBeVisible();
}

/** Clicks the palette entry and waits until rows are on screen; ms. */
function openAndTime(page: Page): Promise<number> {
  return page.evaluate(
    () =>
      new Promise<number>((resolve, reject) => {
        const option = [...document.querySelectorAll<HTMLElement>('[role="option"]')].find((o) => o.textContent?.includes("Git: History"));
        if (!option) return reject(new Error("palette entry not found"));
        const t0 = performance.now();
        option.click();
        const check = () => {
          const rows = document.querySelectorAll('[data-testid="graph-row"]');
          if (rows.length >= 10) {
            // The frame after the rows were inserted is the one that shows them.
            requestAnimationFrame(() => resolve(performance.now() - t0));
          } else if (performance.now() - t0 > 30_000) reject(new Error("no rows in 30 s"));
          else requestAnimationFrame(check);
        };
        requestAnimationFrame(check);
      }),
  );
}

test("git_graph and git_graph_frame: 10 000 commits", async ({ page }) => {
  await page.goto(`/?backend=mock&graph=${COMMITS}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");

  const times: number[] = [];
  for (let i = 0; i <= RUNS; i++) {
    await openPalette(page);
    const ms = await openAndTime(page);
    if (i > 0) times.push(ms);
    await expect(page.getByTestId("git-graph")).toBeVisible();
    if (i < RUNS) {
      await page.getByTestId("git-view-close").click();
      await expect(page.getByTestId("git-graph")).toBeHidden();
    }
  }
  const sorted = [...times].sort((a, b) => a - b);
  const firstRows = Math.round(sorted[Math.floor(sorted.length / 2)]!);

  // Scrolling the whole history.
  const list = page.getByTestId("graph-list");
  await list.focus();
  const raw = await list.evaluate(
    async (el, { frames }) => {
      const frame = () => new Promise<number>((r) => requestAnimationFrame(r));
      const idle: number[] = [];
      let last = await frame();
      for (let i = 0; i < 60; i++) {
        const t = await frame();
        idle.push(t - last);
        last = t;
      }
      let seed = 42;
      const next = () => (seed = (seed * 1103515245 + 12345) % 2 ** 31) / 2 ** 31;
      const key = (k: string) => el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true }));
      const steps: Record<string, () => void> = {
        wheel: () => {
          el.scrollTop += 900;
        },
        jump: () => {
          el.scrollTop = Math.floor(next() * el.scrollHeight);
        },
        keys: () => key("PageDown"),
      };
      const phases: { name: string; intervals: number[] }[] = [];
      for (const [name, step] of Object.entries(steps)) {
        el.scrollTop = 0;
        key("Home");
        await frame();
        last = await frame();
        const intervals: number[] = [];
        for (let i = 0; i < frames; i++) {
          step();
          const t = await frame();
          intervals.push(t - last);
          last = t;
        }
        phases.push({ name, intervals });
      }
      const rows = el.querySelectorAll('[data-testid="graph-row"]').length;
      const loaded = Math.round(el.scrollHeight / 28);
      return { idle, phases, rows, loaded };
    },
    { frames: FRAMES },
  );
  const sort = (a: number[]) => [...a].sort((x, y) => x - y);
  const refreshMs = percentile(sort(raw.idle), 50);
  const phases = raw.phases.map(({ name, intervals }) => {
    const s = sort(intervals);
    return { name, frames: s.length, p50: round(percentile(s, 50)), p95: round(percentile(s, 95)), max: round(s[s.length - 1]!) };
  });
  const all = sort(raw.phases.flatMap((p) => p.intervals));
  const p95 = round(percentile(all, 95));
  // Virtualised: a screenful of rows in the DOM, not thousands; and scrolling loaded every page.
  expect(raw.rows).toBeLessThan(120);
  expect(raw.loaded).toBe(COMMITS);

  writePerfResult("graph", {
    git_graph: firstRows,
    git_graph_frame: p95,
    first_rows: { median: firstRows, runs: times.map(Math.round) },
    frames: { phases, refreshMs: round(refreshMs), dropped: all.filter((d) => d > refreshMs * 1.5).length },
    rendered_rows: raw.rows,
  });
  console.log(`git_graph: median ${firstRows} ms (${times.map(Math.round).join(", ")}); git_graph_frame: p95 ${p95} ms (${phases.map((p) => `${p.name} ${p.p95}`).join(", ")}), ${raw.rows} rows in the DOM`);
});
