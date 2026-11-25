// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand C: the "Next page" chip, "All pages", the paging rule
// dialog and a plugin's rule, against the mock backend. The engine itself is tested in Rust.

import { expect, test, type Page } from "@playwright/test";

async function sendHealth(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

async function setRule(page: Page, merge = "$.items") {
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Paging rule");
  await palette.getByRole("option").filter({ hasText: "Paging rule" }).click();
  await expect(page.getByTestId("paging-dialog")).toBeVisible();
  if (merge) await page.getByTestId("paging-merge").fill(merge);
  await page.getByTestId("paging-save").click();
  await expect(page.getByTestId("paging-dialog")).toHaveCount(0);
}

test("no chip without a rule", async ({ page }) => {
  await sendHealth(page);
  await expect(page.getByTestId("paging-chip")).toHaveCount(0);
});

test("a rule makes the chip appear; Next shows one page, All pages merges within the limit", async ({ page }) => {
  await sendHealth(page);
  await setRule(page);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("paging-chip")).toBeVisible();
  await page.getByTestId("paging-all").click();
  await expect(page.getByTestId("paging-summary")).toContainText("5 pages");
  await expect(page.getByTestId("response-view")).toContainText('"id": 10');
});

test("Next loads the next page as a response of its own", async ({ page }) => {
  await sendHealth(page);
  await setRule(page);
  await page.getByTestId("send").click();
  await page.getByTestId("paging-next").click();
  await expect(page.getByTestId("paging-summary")).toContainText("1 page");
  await expect(page.getByTestId("response-view")).toContainText('"page": 2');
});

test("a bad draft is explained and cannot be saved", async ({ page }) => {
  await sendHealth(page);
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Paging rule");
  await palette.getByRole("option").filter({ hasText: "Paging rule" }).click();
  await page.getByTestId("paging-merge").fill("items");
  await expect(page.getByTestId("paging-problem")).toContainText("merge path");
  await expect(page.getByTestId("paging-save")).toBeDisabled();
});

test("a plugin's rule is only offered, with its name", async ({ page }) => {
  await sendHealth(page, "&paging=plugin");
  await expect(page.getByTestId("paging-chip")).toHaveAttribute("data-origin", "plugin");
  await expect(page.getByTestId("paging-chip")).toContainText("acme");
});
