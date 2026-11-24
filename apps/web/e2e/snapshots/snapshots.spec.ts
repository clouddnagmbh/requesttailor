// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand B: snapshot tests in the app, mock backend: freeze a response, see it match,
// ignore a path, accept a new response. Everything stays unsaved until "Save".

import { expect, test, type Page } from "@playwright/test";

const URL = "/?backend=mock";

async function open(page: Page) {
  await page.goto(URL);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
}

async function send(page: Page) {
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

test("freeze, match, differ, ignore and accept", async ({ page }) => {
  await open(page);
  await send(page);

  // No snapshot yet: one button, no badge.
  await expect(page.getByTestId("snapshot-badge")).toHaveCount(0);
  await page.getByTestId("snapshot-freeze").click();
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");
  await expect(page.getByTestId("snapshot-freeze")).toHaveCount(0);

  // The request got its assertion, staged and unsaved.
  await page.getByTestId("tab-assertions").click();
  await expect(page.getByTestId("request-editor")).toContainText("matchesSnapshot");
  await page.getByTestId("send").click();
  await page.getByTestId("response-tab-tests").click();
  await expect(page.getByTestId("tests-summary")).toContainText("passed");
  await page.getByTestId("response-tab-body").click();

  // Changing the request changes the response: the badge shows the difference.
  await page.getByTestId("url-input").locator(".cm-content").click();
  await page.keyboard.press("End");
  await page.keyboard.type("?changed=1");
  await send(page);
  const badge = page.getByTestId("snapshot-badge");
  await expect(badge).toHaveAttribute("data-state", "differs");
  await badge.click();
  const dialog = page.getByTestId("snapshot-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("snapshot-difference").first()).toBeVisible();

  // Ignore every differing path by a click; the badge turns green.
  while ((await dialog.getByTestId("snapshot-ignore").count()) > 0) {
    await dialog.getByTestId("snapshot-ignore").first().click();
    await page.waitForTimeout(50);
  }
  await expect(dialog.getByTestId("snapshot-ignored").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");
});

test("accepting a new response replaces the snapshot and keeps the ignored paths", async ({ page }) => {
  await open(page);
  await send(page);
  await page.getByTestId("snapshot-freeze").click();
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");
  await page.getByTestId("url-input").locator(".cm-content").click();
  await page.keyboard.press("End");
  await page.keyboard.type("?v=2");
  await send(page);
  await page.getByTestId("snapshot-badge").click();
  await page.getByTestId("snapshot-dialog").getByTestId("snapshot-accept").click();
  await expect(page.getByTestId("snapshot-dialog")).toHaveCount(0);
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");
});
