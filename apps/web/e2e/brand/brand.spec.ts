// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The official marks: the small one in the header, the full logo in the empty state. They must
// load (no broken image), carry the accessible name and not push the header into overflow at a
// narrow width. With `RT_BRAND_SHOTS=<dir>` the test also writes screenshots for review.

import { join } from "node:path";

import { expect, test } from "@playwright/test";

for (const scheme of ["light", "dark"] as const) {
  test(`header mark and start logo render (${scheme})`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await page.setViewportSize({ width: 900, height: 700 });
    await page.goto("/?backend=mock");
    const mark = page.getByTestId("brand-mark");
    const logo = page.getByTestId("brand-logo");
    await expect(mark).toBeVisible();
    await expect(logo).toBeVisible();
    await expect(mark).toHaveAttribute("alt", "RequestTailor");
    await expect(logo).toHaveAttribute("alt", "RequestTailor");
    for (const img of [mark, logo]) {
      expect(await img.evaluate((e: HTMLImageElement) => e.complete && e.naturalWidth > 0)).toBe(true);
    }
    const header = page.locator("header").first();
    expect(await header.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
    const shots = process.env.RT_BRAND_SHOTS;
    if (shots) await page.screenshot({ path: join(shots, `brand-${scheme}.png`) });
  });
}

test("the header does not overflow at a narrow width", async ({ page }) => {
  await page.setViewportSize({ width: 480, height: 700 });
  await page.goto("/?backend=mock");
  await expect(page.getByTestId("brand-logo")).toBeVisible();
  const header = page.locator("header").first();
  expect(await header.evaluate((e) => e.scrollWidth <= e.clientWidth)).toBe(true);
});
