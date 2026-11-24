// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand D: production protection in the app (mock backend, whose environment "PRD" is
// a production name): a red frame and a label, the question before a writing request only,
// Enter confirms and Escape cancels, and it can be switched off on this device.

import { expect, test, type Page } from "@playwright/test";

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const request = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name }).click();
const dialog = (page: Page) => page.getByTestId("confirm-send");

test("outside production there is no frame, the button says Send and nothing is asked", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await request(page, "Create user");
  await expect(page.getByTestId("production-frame")).toHaveCount(0);
  await expect(page.getByTestId("send")).toHaveText(/^Send/);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
});

test("production shows a frame, a badge and 'Send to PRODUCTION'; a reading request is not asked", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await request(page, "List users");
  await expect(page.getByTestId("production-frame")).toBeVisible();
  await expect(page.getByTestId("env-production-badge")).toBeVisible();
  await expect(page.getByTestId("send")).toHaveText(/Send to PRODUCTION/);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await expect(dialog(page)).toHaveCount(0);
  // Back to a safe environment: everything is gone again.
  await page.getByTestId("env-switcher").selectOption("Local");
  await expect(page.getByTestId("production-frame")).toHaveCount(0);
  await expect(page.getByTestId("env-production-badge")).toHaveCount(0);
});

test("a writing request waits for Enter; Escape sends nothing", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(dialog(page)).toBeVisible();
  await expect(dialog(page)).toContainText("Send POST to PRD?");
  await expect(page.getByTestId("confirm-send-summary")).toContainText("{{baseUrl}}/users");
  await expect(page.getByTestId("response-status")).toHaveCount(0);

  await page.keyboard.press("Escape");
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.getByTestId("response-status")).toHaveCount(0);
  await expect(page.getByTestId("send")).toBeEnabled();

  await page.getByTestId("send").click();
  await expect(dialog(page)).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.getByTestId("response-status")).toBeVisible();
});

test("'Don't ask again' switches the question off on this device", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await page.getByTestId("confirm-send-never").check();
  await page.getByTestId("confirm-send-yes").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await page.getByTestId("send").click();
  await expect(dialog(page)).toHaveCount(0);
  await expect(page.getByTestId("production-frame")).toBeVisible();
});
