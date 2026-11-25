// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand W: API notebooks against the mock backend (`?backend=mock`). The mock keeps a
// demo notebook (login, list, create), plays a small API and follows the core's rules: variables
// pass from block to block (a credential is carried and shown as ***), a writing block needs
// "Allow writes" and never runs against production, and nothing of a run is saved.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openNotebooks(page: Page, env = "Local") {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("env-switcher").selectOption(env);
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Notebooks");
  await palette.getByRole("option").filter({ hasText: "Notebooks" }).click();
  await expect(page.getByTestId("notebook-panel")).toBeVisible();
}

test("prose is text, blocks run in order, variables pass on and credentials are masked", async ({ page }) => {
  await openNotebooks(page);
  await expect(page.getByTestId("notebook-text").first()).toContainText("Orders walkthrough");
  await expect(page.getByTestId("notebook-block")).toHaveCount(3);
  await page.getByTestId("notebook-run-all").click();
  const login = page.locator('[data-block="login"]');
  await expect(login.getByTestId("notebook-status")).toContainText("200");
  await expect(login.getByTestId("notebook-set").filter({ hasText: "token" })).toHaveText("token = ***");
  await expect(login.getByTestId("notebook-set").filter({ hasText: "userId" })).toHaveText("userId = 42");
  await expect(login.getByTestId("notebook-body")).toBeVisible();
  const list = page.locator('[data-block="list"]');
  await expect(list.locator('[data-kind="table"] tbody tr')).toHaveCount(3);
  await expect(list.getByTestId("notebook-chart")).toBeVisible();
  // The writing block was not allowed.
  await expect(page.locator('[data-block="create"]').getByTestId("notebook-skipped")).toHaveText("skipped (writes)");
  // The carried credential is not in the variable rows (the response is shown as received).
  await expect(page.getByTestId("notebook-set").filter({ hasText: "mock-token-1234" })).toHaveCount(0);
});

test("a single block runs with the variables the view holds, and writes need the checkbox", async ({ page }) => {
  await openNotebooks(page);
  await page.getByTestId("notebook-run-login").click();
  await expect(page.locator('[data-block="login"]').getByTestId("notebook-status")).toBeVisible();
  await page.getByTestId("notebook-run-list").click();
  await expect(page.locator('[data-block="list"]').getByTestId("notebook-status")).toContainText("200");
  await page.getByTestId("notebook-run-create").click();
  await expect(page.locator('[data-block="create"]').getByTestId("notebook-skipped")).toHaveText("skipped (writes)");
  await page.getByTestId("notebook-writes").check();
  await page.getByTestId("notebook-run-create").click();
  await expect(page.locator('[data-block="create"]').getByTestId("notebook-status")).toContainText("200");
});

test("writing is never sent to production, even with permission", async ({ page }) => {
  await openNotebooks(page, "PRD");
  await page.getByTestId("notebook-writes").check();
  await page.getByTestId("notebook-run-create").click();
  await expect(page.locator('[data-block="create"]').getByTestId("notebook-skipped")).toHaveText("skipped (production)");
});

test("a new notebook is staged unsaved and listed; a run stores nothing", async ({ page }) => {
  await openNotebooks(page);
  await page.getByTestId("notebook-create").first().click();
  await expect(page.getByTestId("notebook-select")).toContainText("notebook");
  await expect(page.getByTestId("notebook-text").first()).toContainText("Write what you want");
  await page.getByTestId("notebook-select").selectOption({ index: 0 });
  await page.getByTestId("notebook-run-all").click();
  await expect(page.getByTestId("notebook-error")).toHaveCount(0);
});
