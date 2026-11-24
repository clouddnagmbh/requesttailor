// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: the one-line timing bar in the response's status line and in the history
// rows (mock backend).

import { expect, test } from "@playwright/test";

test("the status line shows one bar and the share of server wait; history rows show a bar", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();

  const bar = page.getByTestId("timing-bar");
  await expect(bar).toBeVisible();
  await expect(page.getByTestId("timing-bar-sentence")).toHaveText(/^Server wait is \d+ % of [\d.]+ m?s$/);
  const segments = await bar.locator(".seg").count();
  expect(segments).toBeGreaterThanOrEqual(2);
  expect(await bar.getAttribute("title")).toContain("Server wait");
  // One line: the bar and the status line share a row with the time.
  const time = (await page.getByTestId("response-time").boundingBox())!;
  const barBox = (await bar.boundingBox())!;
  expect(Math.abs(barBox.y + barBox.height / 2 - (time.y + time.height / 2))).toBeLessThan(12);
  // The waterfall tab still has the detail.
  await expect(page.getByTestId("response-timing")).toHaveCount(0);

  await page.getByTestId("view-history").click();
  await expect(page.getByTestId("history-entry").first().getByTestId("history-timing-bar")).toBeVisible();
});
