// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand F: the lint view (palette "Lint collection"), the health
// score, findings by rule and by folder, clicking a finding opens the request, "Ignore this",
// the level per rule, and no horizontal scrolling at 900 px. Mock backend.

import { expect, test, type Page } from "@playwright/test";

async function openLint(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Lint collection");
  await palette.getByRole("option").filter({ hasText: "Lint collection" }).click();
  await expect(page.getByTestId("lint")).toBeVisible();
  await expect(page.getByTestId("lint-score")).toBeVisible();
}

const findings = (page: Page) => page.getByTestId("lint-finding");
const rule = (page: Page, name: string) => page.locator(`[data-testid=lint-finding][data-rule="${name}"]`);

test("the palette opens the lint with a score and the findings of the demo collection", async ({ page }) => {
  await openLint(page);
  const score = Number((await page.getByTestId("lint-score").locator(".number").innerText()).trim());
  expect(score).toBeGreaterThan(0);
  expect(score).toBeLessThan(100);
  // The demo requests have no tests; the blocked one has a fixed host; two variables are unused.
  await expect(rule(page, "no-tests")).toHaveCount(5);
  await expect(rule(page, "hard-coded-host")).toHaveCount(1);
  await expect(rule(page, "hard-coded-host").first()).toContainText("intranet.example");
  await expect(rule(page, "orphan-variable")).toHaveCount(2);
  await expect(rule(page, "duplicate-name")).toHaveCount(0);
  // Every rule shows its numbers.
  for (const name of ["no-tests", "hard-coded-host", "orphan-variable", "duplicate-name"]) {
    await expect(page.getByTestId(`lint-rule-${name}`)).toBeVisible();
  }
  await expect(page.getByTestId("lint-rule-no-tests")).toContainText("0 / 5");
});

test("the formula is explained", async ({ page }) => {
  await openLint(page);
  await page.getByText("How is this calculated?").click();
  await expect(page.getByTestId("lint")).toContainText("weighted mean");
});

test("findings can be grouped by folder", async ({ page }) => {
  await openLint(page);
  await page.getByRole("radio", { name: "By folder" }).click();
  await expect(findings(page).first()).toBeVisible();
  await expect(page.getByTestId("lint").getByRole("heading", { name: /^users/ })).toBeVisible();
  await expect(page.getByTestId("lint").getByRole("heading", { name: /^Collection/ })).toBeVisible();
});

test("clicking a request finding opens the request", async ({ page }) => {
  await openLint(page);
  await rule(page, "hard-coded-host").first().getByRole("button", { name: "demo/blocked.yml" }).click();
  await expect(page.getByTestId("lint")).toHaveCount(0);
  await expect(page.getByTestId("tab").filter({ hasText: "Blocked by the browser" })).toBeVisible();
});

test("Ignore this leaves the finding out and counts it", async ({ page }) => {
  await openLint(page);
  const before = await findings(page).count();
  await rule(page, "hard-coded-host").first().getByTestId("lint-ignore").click();
  await expect(rule(page, "hard-coded-host")).toHaveCount(0);
  await expect(findings(page)).toHaveCount(before - 1);
  await expect(page.getByTestId("lint")).toContainText("1 ignored");
  // The other rules still report their findings for that request.
  await expect(rule(page, "no-tests")).toHaveCount(5);
});

test("switching a rule off removes its findings and its weight", async ({ page }) => {
  await openLint(page);
  await page.getByTestId("lint-level-no-tests").selectOption("off");
  await expect(rule(page, "no-tests")).toHaveCount(0);
  await expect(page.getByTestId("lint-rule-no-tests")).toContainText("nothing to check");
  await page.getByTestId("lint-level-hard-coded-host").selectOption("error");
  await expect(rule(page, "hard-coded-host").first()).toHaveAttribute("data-level", "error");
});

test("the view does not scroll sideways at 900 px", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await openLint(page);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  const inner = await page.getByTestId("lint").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(inner).toBeLessThanOrEqual(0);
});
