// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Budget `git_graph_large`, the view's part (wave 15, strand C): a repository with 100 000
// commits (mock backend, `?graph=100000`), from opening "Git: History" until its first rows are
// on screen, ms, median of five openings after one warm-up. The native part (Git reading the
// first page) is `crates/rt-git/examples/perf_graph.rs`; `scripts/perf/graph-large.mjs` adds
// both. The view must not hold the whole history: a screenful of rows in the DOM, one page loaded.

import { expect, test, type Page } from "@playwright/test";

import { writePerfResult } from "./frames";

const MOD = "ControlOrMeta";
const COMMITS = 100_000;
const RUNS = 5;

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
          if (rows.length >= 10) requestAnimationFrame(() => resolve(performance.now() - t0));
          else if (performance.now() - t0 > 30_000) reject(new Error("no rows in 30 s"));
          else requestAnimationFrame(check);
        };
        requestAnimationFrame(check);
      }),
  );
}

test("git_graph_large (view): 100 000 commits", async ({ page }) => {
  await page.goto(`/?backend=mock&graph=${COMMITS}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");

  const times: number[] = [];
  let rendered = 0;
  for (let i = 0; i <= RUNS; i++) {
    await openPalette(page);
    const ms = await openAndTime(page);
    if (i > 0) times.push(ms);
    await expect(page.getByTestId("git-graph")).toBeVisible();
    rendered = await page.locator('[data-testid="graph-row"]').count();
    if (i < RUNS) {
      await page.getByTestId("git-view-close").click();
      await expect(page.getByTestId("git-graph")).toBeHidden();
    }
  }
  const sorted = [...times].sort((a, b) => a - b);
  const median = Math.round(sorted[Math.floor(sorted.length / 2)]! * 100) / 100;
  expect(rendered).toBeLessThan(120);
  writePerfResult("graph-large", { first_rows_ms: median, runs: times.map((t) => Math.round(t * 100) / 100), rendered_rows: rendered });
  console.log(`git_graph_large (view): median ${median} ms (${times.map(Math.round).join(", ")}), ${rendered} rows in the DOM`);
});
