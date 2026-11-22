// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Budget `large_response_frame`: a 100 MB response renders and scrolls without stutter.
// Two runs: the mock backend's virtual 100 MB body (`demo/large.yml`, UI only) and the real
// core (rt-wasm) holding a 100 MB body fetched from rt-testserver. Only measures; the gate is
// `node scripts/check-perf-budgets.mjs` over what `scripts/perf/run.mjs` collects.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder } from "../shells/env";
import { measureControl, measureScroll, writePerfResult, type ScrollRun } from "./frames";

const HUNDRED_MB = 100 * 1024 * 1024;

test.describe.configure({ mode: "serial" });

function report(name: string, run: ScrollRun) {
  const { intervals: _all, ...summary } = run;
  writePerfResult(`frames-${name}`, summary);
  test.info().annotations.push({ type: `p95 frame (${name})`, description: `${run.p95} ms` });
  console.log(`frames ${name}: p95 ${run.p95} ms, dropped ${run.dropped}/${run.intervals.length}, refresh ${run.refreshMs} ms`);
  for (const p of run.phases) console.log(`  ${p.name.padEnd(6)} p50 ${p.p50} p95 ${p.p95} max ${p.max} (${p.frames} frames)`);
}

async function scrolled(page: Page): Promise<number> {
  return Number(await page.getByTestId("response-scrollbar").getAttribute("aria-valuenow"));
}

test("mock backend: virtual 100 MB body", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "100 MB response" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-size")).toHaveText("100 MB");
  const viewport = page.getByTestId("response-lines");
  await expect(viewport.getByTestId("response-line").first()).toContainText('{"line": 000000000, ');

  const run = await measureScroll(page, viewport);
  expect(await scrolled(page)).toBeGreaterThan(0);
  report("mock", run);

  // The method sees stutter: frames with 25 ms of work show up as long intervals.
  const control = await measureControl(page);
  console.log(`frames control (25 ms of work per frame): p95 ${control.toFixed(1)} ms`);
  expect(control).toBeGreaterThan(25);
});

test("real core: 100 MB body from rt-testserver", async ({ page }) => {
  test.setTimeout(180_000);
  await fakePicker(page, "perf");
  await page.goto("/");
  await fillFolder(
    page,
    {
      "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Perf\n',
      "large.yml": `info:\n  name: Large\n  type: http\n  seq: 1\n\nhttp:\n  method: GET\n  url: "${SERVER}/bytes/${HUNDRED_MB}"\n`,
    },
    "perf",
  );
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Perf");
  await page.getByTestId("tree-request").filter({ hasText: "Large" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200", { timeout: 120_000 });
  await expect(page.getByTestId("response-size")).toHaveText("100 MB");
  const viewport = page.getByTestId("response-lines");
  await expect(viewport.getByTestId("response-line").first()).toBeVisible();

  const run = await measureScroll(page, viewport);
  expect(await scrolled(page)).toBeGreaterThan(0);
  report("core", run);
});
