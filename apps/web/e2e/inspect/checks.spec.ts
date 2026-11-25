// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand B (point 3): the chip "n checks" in the status line and the
// "Checks" tab of the response, each observation with where, why and fix, a rule switched off per
// device or per collection, and nothing leaves the page for it. Mock backend.

import { expect, test, type Page } from "@playwright/test";

async function sendList(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

const chip = (page: Page) => page.getByTestId("checks-chip");
const items = (page: Page) => page.getByTestId("checks-item");
const countOf = async (page: Page) => Number(((await chip(page).innerText()).match(/(\d+) check/) ?? [])[1]);

test("a response gets a calm chip that counts the observations", async ({ page }) => {
  await sendList(page);
  await expect(chip(page)).toBeVisible();
  await expect(chip(page)).toContainText(/\d+ checks?/);
  // The demo's JSON answer has no X-Content-Type-Options.
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  await expect(page.getByTestId("checks-panel")).toBeVisible();
  const nosniff = page.locator('[data-testid=checks-item][data-id="chk/nosniff-missing"]');
  await expect(nosniff).toBeVisible();
  await expect(nosniff).toContainText("Send X-Content-Type-Options: nosniff.");
  await expect(nosniff).toContainText("response headers");
  await expect(nosniff).toHaveAttribute("data-severity", "info");
  expect(await items(page).count()).toBe(await countOf(page));
});

test("the chip opens the same list in a dialog", async ({ page }) => {
  await sendList(page);
  await chip(page).getByRole("button").click();
  const dialog = page.getByTestId("extension-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("checks-item").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
});

test("a rule can be turned off on this device, and the chip follows", async ({ page }) => {
  await sendList(page);
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  const before = await items(page).count();
  expect(before).toBeGreaterThan(0);
  await items(page).first().getByTestId("checks-off-device").click();
  await expect(items(page)).toHaveCount(before - 1);
  if (before - 1 > 0) expect(await countOf(page)).toBe(before - 1);
  else await expect(chip(page)).toHaveCount(0);
});

test("a rule can be turned off for the collection", async ({ page }) => {
  await sendList(page);
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  const first = items(page).first();
  const id = await first.getAttribute("data-id");
  await first.getByTestId("checks-off-collection").click();
  await expect(page.locator(`[data-testid=checks-item][data-id="${id}"]`)).toHaveCount(0);
});

test("looking at the checks sends nothing and asks nobody", async ({ page }) => {
  await sendList(page);
  const seen: string[] = [];
  page.on("request", (r) => seen.push(r.url()));
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  await expect(page.getByTestId("checks-panel")).toBeVisible();
  await chip(page).getByRole("button").click();
  await page.keyboard.press("Escape");
  const outside = seen.filter((u) => !u.startsWith("http://127.0.0.1") && !u.startsWith("data:") && !u.startsWith("blob:"));
  expect(outside).toEqual([]);
});

test("the tab fits a narrow window without a horizontal scrollbar", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await sendList(page);
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  const panel = page.getByTestId("checks-panel");
  await expect(panel).toBeVisible();
  const overflow = await panel.evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});
