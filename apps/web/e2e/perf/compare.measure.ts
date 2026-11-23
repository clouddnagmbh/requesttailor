// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Budget `compare_view` (wave 14, strand H): two revisions of a collection with 1000 requests,
// 100 of them changed, compared in the view, until the result is visible: the numbers, the
// list, the first change side by side and the answers of both runs. Mock backend (the view, not
// a network). Median of five comparisons after one warm-up. Only measures; the gate is
// `node scripts/check-perf-budgets.mjs` over what `scripts/perf/run.mjs` collects.

import { expect, test } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

import { writePerfResult } from "./frames";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";
const REQUESTS = 1000;
const CHANGED = 100;
const RUNS = 5;

test("compare_view: 1000 requests, 100 changed", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(
    async ({ requests, changed }) => {
      const b = (window as unknown as W).__rt.backend;
      await b.git.setIdentity({ name: "Perf", email: "perf@example.com" });
      const paths: string[] = [];
      for (let i = 0; i < requests; i++) {
        const folder = await b.createFolder(`group-${String(i % 20).padStart(2, "0")}`);
        const path = await b.createRequest(folder, `Request ${String(i).padStart(4, "0")}`);
        const spec = await b.getRequest(path);
        await b.updateRequest(path, { ...spec, url: `{{baseUrl}}/items/${i}` });
        paths.push(path);
      }
      await b.save();
      await b.git.commit("A thousand requests", paths);
      const edited = paths.filter((_, i) => i % (requests / changed) === 0);
      for (const path of edited) {
        const spec = await b.getRequest(path);
        await b.updateRequest(path, { ...spec, url: `${spec.url}/v2`, headers: [{ name: "X-Version", value: "2", enabled: true }] });
      }
      await b.save();
      await b.git.commit("A hundred changed", edited);
    },
    { requests: REQUESTS, changed: CHANGED },
  );

  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("compare rev");
  await dialog.getByRole("option").filter({ hasText: "Compare revisions" }).click();
  await page.getByTestId("compare-from").fill("HEAD~1");
  await page.getByTestId("compare-to").fill("HEAD");
  await page.keyboard.press("Escape");

  const once = () =>
    page.evaluate(
      ({ changed }) =>
        new Promise<number>((resolve, reject) => {
          const q = (id: string) => document.querySelector(`[data-testid="${id}"]`);
          const t0 = performance.now();
          (q("compare-start") as HTMLButtonElement).click();
          const check = () => {
            const value = (id: string) => q(id)?.querySelector(".value")?.textContent?.trim();
            const done =
              value("compare-count-changed") === String(changed) &&
              value("compare-count-failures") !== "–" &&
              !q("compare-running") &&
              q("compare-row") !== null &&
              q("cmp-response") !== null;
            if (done) resolve(performance.now() - t0);
            else if (performance.now() - t0 > 30_000) reject(new Error("the comparison did not appear in 30 s"));
            else requestAnimationFrame(check);
          };
          requestAnimationFrame(check);
        }),
      { changed: CHANGED },
    );

  const first = Math.round(await once()); // warm-up, reported
  const times: number[] = [];
  for (let i = 0; i < RUNS; i++) times.push(await once());
  const sorted = [...times].sort((a, b) => a - b);
  const median = Math.round(sorted[Math.floor(sorted.length / 2)]!);
  // Every request is listed: the "All" filter shows a thousand rows, virtualized.
  await page.getByTestId("compare-filter-all").click();
  await expect(page.getByTestId("compare-panel")).toContainText(`of ${REQUESTS + 5}`);
  const rendered = await page.getByTestId("compare-row").count();
  expect(rendered).toBeLessThan(80);
  writePerfResult("compare", { compare_view: median, first, runs: times.map(Math.round), rendered_rows: rendered });
  console.log(`compare_view: median ${median} ms, first ${first} ms (${times.map(Math.round).join(", ")}), ${rendered} rows in the DOM`);
});
