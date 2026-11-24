// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: comparing environments against the mock backend (`?backend=mock`). Local
// and PRD answer the same requests; the mock echoes the resolved URL, so they differ until the
// environments' own values are ignored. Writing requests are named, not sent; PRD asks once.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCompare(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("compare environments");
  await dialog.getByRole("option").filter({ hasText: "Compare environments" }).click();
  await expect(page.getByTestId("envcompare-panel")).toBeVisible();
}

test("compares two environments, names the skipped writes and asks once for production", async ({ page }) => {
  await openCompare(page);
  // The active environment and one more are chosen; PRD is marked.
  await expect(page.getByTestId("env-option")).toHaveCount(2);
  await expect(page.getByTestId("env-reference")).toBeVisible();
  await expect(page.getByTestId("env-picker").getByText("PRODUCTION")).toBeVisible();
  await expect(page.getByTestId("envcompare-intro")).toBeVisible();

  // Esc on the question sends nothing.
  await page.getByTestId("envcompare-start").click();
  await expect(page.getByTestId("envcompare-confirm")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("envcompare-confirm")).toBeHidden();
  await expect(page.getByTestId("envcompare-result")).toHaveCount(0);

  // Enter confirms (the confirm button has the focus).
  await page.getByTestId("envcompare-start").click();
  await expect(page.getByTestId("envcompare-confirm-yes")).toBeFocused();
  await page.keyboard.press("Enter");
  const summary = page.getByTestId("envcompare-summary");
  await expect(summary).toBeVisible();
  await expect(summary).toContainText("(reference) against");
  expect(Number(await summary.getAttribute("data-different"))).toBeGreaterThan(0);
  // The writing request is named with its reason, and it is not in the list.
  await expect(page.getByTestId("envcompare-skipped")).toContainText("Create user");
  await expect(page.getByTestId("envcompare-skipped")).toContainText("skipped (writes)");
  await expect(page.getByTestId("envcompare-row").filter({ hasText: "Create user" })).toHaveCount(0);
  // The first difference is open, both answers side by side.
  await expect(page.getByTestId("envcompare-detail")).toBeVisible();
  await expect(page.getByTestId("cmp-response")).toBeVisible();
  await expect(page.getByTestId("envcompare-legend")).toContainText("(reference)");
});

test("ignoring the values of environment variables hides what only the hosts cause", async ({ page }) => {
  await openCompare(page);
  await page.getByTestId("envcompare-ignore").check();
  await page.getByTestId("envcompare-start").click();
  await page.getByTestId("envcompare-confirm-yes").click();
  const summary = page.getByTestId("envcompare-summary");
  await expect(summary).toBeVisible();
  await expect(summary).toHaveAttribute("data-different", "0");
  await expect(page.getByTestId("envcompare-empty")).toContainText("No differences");
  // "All" lists every request that was sent.
  await page.getByTestId("envcompare-filter-all").click();
  expect(await page.getByTestId("envcompare-row").count()).toBeGreaterThan(0);
});

test("n and p jump between differences", async ({ page }) => {
  await openCompare(page);
  await page.getByTestId("envcompare-start").click();
  await page.getByTestId("envcompare-confirm-yes").click();
  await expect(page.getByTestId("envcompare-detail")).toBeVisible();
  const first = await page.getByTestId("envcompare-detail").getAttribute("data-path");
  await page.keyboard.press("n");
  await expect(page.getByTestId("envcompare-detail")).not.toHaveAttribute("data-path", first!);
  await page.keyboard.press("p");
  await expect(page.getByTestId("envcompare-detail")).toHaveAttribute("data-path", first!);
});
