// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand A: the "Trace id" chip with Copy, and the segment tree under the waterfall
// (mock backend; `?trace=1` mirrors a collection with `trace.inject`, `extensions=demo19`
// contributes segments like an extension would).

import { expect, test, type Page } from "@playwright/test";

async function send(page: Page, url: string) {
  await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto(url);
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

test("tracing off: no chip and no traceparent in the request", async ({ page }) => {
  await send(page, "/?backend=mock");
  await expect(page.getByTestId("trace-chip")).toHaveCount(0);
  await page.getByTestId("response-tab-headers").click();
  const body = await page.evaluate(() => document.body.innerText);
  expect(body).not.toContain("traceparent");
});

test("tracing on: the chip shows the id and Copy puts the whole id on the clipboard", async ({ page }) => {
  await send(page, "/?backend=mock&trace=1");
  const chip = page.getByTestId("trace-chip");
  await expect(chip).toBeVisible();
  const id = (await page.getByTestId("trace-id").getAttribute("data-trace-id"))!;
  expect(id).toMatch(/^[0-9a-f]{32}$/);
  await expect(page.getByTestId("trace-id")).toHaveText(`${id.slice(0, 4)}…`);
  await page.getByTestId("trace-copy").click();
  await expect(page.getByTestId("trace-copy")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(id);
  // The request that went out carries the same id.
  await page.getByTestId("response-tab-headers").click();
  await expect(page.getByTestId("request-headers")).toContainText(`00-${id}-`);
});

test("segments contributed by an extension show as a tree under the waterfall, not in the sum", async ({ page }) => {
  await send(page, "/?backend=mock&extensions=demo19");
  await page.getByTestId("response-tab-timing").click();
  const block = page.getByTestId("segments-trace");
  await expect(block).toBeVisible();
  await expect(block).toContainText("as reported by trace");
  const rows = block.locator("tr");
  await expect(rows).toHaveCount(3);
  await expect(rows.nth(0)).toHaveAttribute("data-depth", "0");
  await expect(rows.nth(1)).toHaveAttribute("data-depth", "1");
  await expect(rows.nth(2)).toHaveAttribute("data-depth", "2");
  await expect(rows.nth(2)).toContainText("SELECT orders");
  await expect(block).toContainText("Not part of the total");
  // The waterfall's own total is untouched by the segments.
  await expect(page.getByTestId("response-timing")).toContainText("Total");
  // The tree is indented: the database row starts further right than the gateway row.
  const pad = (n: number) => rows.nth(n).locator("th").evaluate((el) => parseFloat(getComputedStyle(el).paddingLeft));
  expect(await pad(2)).toBeGreaterThan(await pad(0));
});
