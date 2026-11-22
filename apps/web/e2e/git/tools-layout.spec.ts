// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The Git panel's "Tools" labels wrap inside the panel instead of being cut off, at the default
// sidebar width and in a 900 px window (mock backend).

import { expect, test, type Page } from "@playwright/test";

async function check(page: Page, name: string) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("view-git").click();
  const tools = page.getByTestId("git-tools");
  await expect(tools).toBeVisible();
  const panel = (await tools.evaluate((el) => el.closest("section")!.parentElement!.getBoundingClientRect().right)) as number;
  const buttons = tools.locator("button");
  const n = await buttons.count();
  expect(n).toBeGreaterThan(0);
  for (let i = 0; i < n; i++) {
    const b = buttons.nth(i);
    const box = (await b.boundingBox())!;
    // Nothing is cut: the text fits in the button and the button in the panel.
    expect(await b.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
    expect(box.x + box.width).toBeLessThanOrEqual(panel + 1);
    expect(await b.getAttribute("title")).toBeTruthy();
  }
  const scroller = await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
  expect(scroller).toBe(true);
  await page.screenshot({ path: test.info().outputPath(`git-tools-${name}.png`) });
}

test("tools labels fit at the default sidebar width", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 800 });
  await check(page, "default");
});

test("tools labels fit in a 900 px window", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await check(page, "900");
});
