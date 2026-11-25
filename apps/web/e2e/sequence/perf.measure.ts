// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Budget `sequence_render` (wave 19, strand G), the browser half: the layout and first draw of a
// diagram of 500 messages (median of five reloads, at most 100 ms) and the scroll of 2 000
// messages between 8 participants (p95 gap between animation frames at most 20 ms). The mock
// backend answers the model at once, so what is timed is the view. The file is named
// `.measure.ts` so the normal e2e run does not time anything under load; the gate is
// `node scripts/check-perf-budgets.mjs` over what `scripts/perf/run.mjs --only sequence` collects.

import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function open(page: Page, query: string) {
  await page.goto(`/?backend=mock&sequence=${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("Sequence diagram: this session");
  await dialog.getByRole("option").filter({ hasText: "Sequence diagram: this session" }).first().click();
  await expect(page.getByTestId("sequence-svg")).toBeVisible();
}

test("sequence_render: layout of 500 messages and scroll of 2000", async ({ page }) => {
  await open(page, "500");
  await expect(page.getByTestId("sequence-foot")).toContainText("500 messages, 8 participants");
  // Switching "Include query" reads the model again: layout and draw of 500 messages.
  const layouts: number[] = [];
  for (let i = 0; i < 6; i++) {
    const expected = i % 2 === 0;
    const ms = await page.evaluate(
      (withQuery) =>
        new Promise<number>((resolve, reject) => {
          const box = document.querySelector<HTMLInputElement>('[data-testid="sequence-query"]')!;
          const t0 = performance.now();
          box.click();
          const check = () => {
            const svg = document.querySelector('[data-testid="sequence-svg"]');
            if (svg && svg.textContent?.includes("?page=") === withQuery) {
              requestAnimationFrame(() => resolve(performance.now() - t0));
              return;
            }
            if (performance.now() - t0 > 10_000) reject(new Error("the diagram did not change"));
            else requestAnimationFrame(check);
          };
          requestAnimationFrame(check);
        }),
      expected,
    );
    if (i > 0) layouts.push(ms);
  }
  layouts.sort((a, b) => a - b);
  const layout = layouts[Math.floor(layouts.length / 2)] ?? 0;

  await open(page, "big");
  await expect(page.getByTestId("sequence-foot")).toContainText("2000 messages, 8 participants");
  const intervals = await page.evaluate(
    () =>
      new Promise<number[]>((resolve) => {
        const el = document.querySelector<HTMLElement>('[data-testid="sequence-diagram"]')!;
        const out: number[] = [];
        let last = performance.now();
        let frame = 0;
        const step = () => {
          const now = performance.now();
          out.push(now - last);
          last = now;
          el.scrollTop += 600;
          if (++frame < 240) requestAnimationFrame(step);
          else resolve(out);
        };
        requestAnimationFrame(step);
      }),
  );
  const sorted = intervals.slice(1).sort((a, b) => a - b);
  const p95 = sorted[Math.min(sorted.length - 1, Math.ceil(0.95 * sorted.length) - 1)] ?? 0;
  const result = { layout_ms: Math.round(layout * 100) / 100, scroll_p95_ms: Math.round(p95 * 100) / 100, scroll_frames: sorted.length };
  console.log(JSON.stringify(result));
  const out = process.env.RT_PERF_OUT;
  if (out) {
    mkdirSync(out, { recursive: true });
    writeFileSync(join(out, "sequence-browser.json"), JSON.stringify(result));
  }
  expect(p95).toBeLessThanOrEqual(20);
});
