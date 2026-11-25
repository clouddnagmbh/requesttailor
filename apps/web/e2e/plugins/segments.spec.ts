// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand J: what the server reported about its own time (`SendReport.segments`, from the
// OData plugin's `sap-statistics` reader) is one line under the timing bar, "as reported by the
// server", and never part of the total (mock backend; `?segments=1` mirrors a plugin that
// reported segments). The numbers come from the server's clock.

import { expect, test, type Page } from "@playwright/test";

async function send(page: Page, url: string) {
  await page.goto(url);
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

test("without segments there is no second line", async ({ page }) => {
  await send(page, "/?backend=mock");
  await expect(page.getByTestId("timing-bar")).toBeVisible();
  await expect(page.getByTestId("timing-segments")).toHaveCount(0);
});

test("the server's own time is one line under the bar and not part of the total", async ({ page }) => {
  await send(page, "/?backend=mock&segments=1");
  const line = page.getByTestId("timing-segments");
  await expect(line).toBeVisible();
  await expect(line).toContainText("as reported by the server");
  await expect(line).toContainText("Gateway 12 ms");
  await expect(line).toContainText("Application 30 ms");
  await expect(line).toContainText("Database 8 ms");
  await expect(line).toContainText("Other 3 ms");

  // Under the bar, not beside it.
  const bar = (await page.getByTestId("timing-bar").locator(".bar").boundingBox())!;
  const text = (await line.boundingBox())!;
  expect(text.y).toBeGreaterThanOrEqual(bar.y + bar.height - 1);

  // The bar and its sentence still describe the client's own total: the segments (53 ms) are
  // not added to it, the share of server wait is unchanged by them.
  const sentence = (await page.getByTestId("timing-bar-sentence").textContent())!;
  const shares = await page.getByTestId("timing-bar").locator(".seg").evaluateAll((els) =>
    els.map((e) => parseFloat((e as HTMLElement).style.width)),
  );
  expect(shares.reduce((a, b) => a + b, 0)).toBeCloseTo(100, 0);
  expect(sentence).toMatch(/^Server wait is \d+ % of [\d.]+ m?s$/);

  // The waterfall tab has the tree with the same numbers.
  await page.getByTestId("response-tab-timing").click();
  const block = page.getByTestId("segments-server");
  await expect(block).toContainText("as reported by server");
  await expect(block).toContainText("Not part of the total");
});
