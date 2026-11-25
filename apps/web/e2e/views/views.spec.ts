// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand G (point 9): "Save as view" next to the response filter,
// the saved views as a tab, a table with chosen columns, the file staged next to the request
// (unsaved until the collection is saved) with the expression only, and a view that is still
// there for the next response. Mock backend.

import { expect, test, type Page } from "@playwright/test";

async function sendTo(page: Page, path: string) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  const url = page.getByTestId("url-input").locator(".cm-content");
  await url.click();
  await url.press("ControlOrMeta+a");
  await page.keyboard.type(`{{baseUrl}}/${path}`);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

async function filter(page: Page, expression: string) {
  const box = page.getByTestId("response-filter");
  await box.fill(expression);
  await box.press("Enter");
  await expect(page.getByTestId("response-filter-result")).toBeVisible();
}

async function saveView(page: Page, name: string, display: "JSON" | "Table" | "Text", columns?: string) {
  await page.getByTestId("save-as-view").click();
  await expect(page.getByTestId("save-view-dialog")).toBeVisible();
  await page.getByTestId("save-view-name").fill(name);
  await page.getByTestId("save-view-display").getByRole("radio", { name: display }).click();
  if (columns !== undefined) await page.getByTestId("save-view-columns").fill(columns);
  await page.getByTestId("save-view-save").click();
}

test("a filter becomes a saved view and shows as a tab", async ({ page }) => {
  await sendTo(page, "json-compact");
  await filter(page, "$.price");
  await saveView(page, "Price", "Text");
  await expect(page.getByTestId("views-panel")).toBeVisible();
  await expect(page.getByTestId("views-pick")).toHaveText("Price");
  await expect(page.getByTestId("views-expression")).toHaveText("$.price");
  await expect(page.getByTestId("views-text")).toHaveText("1");
});

test("a table shows the chosen columns", async ({ page }) => {
  await sendTo(page, "json-compact");
  await filter(page, "$.items");
  await saveView(page, "Items", "Table", "name, missing");
  const table = page.getByTestId("views-table");
  await expect(table.locator("th")).toHaveText(["name", "missing"]);
  await expect(page.getByTestId("views-row")).toHaveCount(2);
  await expect(page.getByTestId("views-row").first().locator("td").first()).toHaveText("a");
});

test("the view stays for the next response", async ({ page }) => {
  await sendTo(page, "json-compact");
  await filter(page, "$.empty");
  await saveView(page, "Empty", "JSON");
  await expect(page.getByTestId("views-json")).toBeVisible();
  // Sending again: the tab is still there with the view.
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.getByTestId("response-tab-slot-views").click();
  await expect(page.getByTestId("views-pick")).toHaveText("Empty");
  await expect(page.getByTestId("views-json")).toBeVisible();
});

test("a view can be removed and the tab explains how to make one", async ({ page }) => {
  await sendTo(page, "json-compact");
  await filter(page, "$.id");
  await saveView(page, "Id", "JSON");
  await page.getByTestId("views-remove").click();
  await expect(page.getByTestId("views-empty")).toBeVisible();
  await expect(page.getByTestId("views-empty")).toContainText("Save as view");
});

test("a name that exists is replaced and said so", async ({ page }) => {
  await sendTo(page, "json-compact");
  await filter(page, "$.id");
  await saveView(page, "Same", "JSON");
  await page.getByTestId("response-tab-body").click();
  await filter(page, "$.items");
  await page.getByTestId("save-as-view").click();
  await page.getByTestId("save-view-name").fill("Same");
  await expect(page.getByTestId("save-view-replaces")).toBeVisible();
  await page.getByTestId("save-view-save").click();
  await expect(page.getByTestId("views-pick")).toHaveCount(1);
  await expect(page.getByTestId("views-expression")).toHaveText("$.items");
});

test("an HTML response has no views tab", async ({ page }) => {
  await sendTo(page, "html");
  await expect(page.getByTestId("response-tab-slot-views")).toHaveCount(0);
});
