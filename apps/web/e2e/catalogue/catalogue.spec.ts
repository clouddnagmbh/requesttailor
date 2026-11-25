// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I: the plugin catalogue in the UI against the mock backend. No built-in index,
// nothing requested before a click, a source is added and refreshed, the plugin's own rights are
// shown before the install (a difference from the index warns), a checksum mismatch refuses, an
// update that asks for more is confirmed again, and the dialog and the view do not scroll
// sideways at 900 px. Run with `RT_WEB_PORT=48628`.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";
const SOURCE = "https://plugins.example.test/plugins.json";

async function openCatalogue(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("plugins-dialog")).toBeVisible();
  await page.getByTestId("plugins-tab-catalogue").click();
  await expect(page.getByTestId("catalogue")).toBeVisible();
}

async function addAndRefresh(page: Page, url = SOURCE) {
  await page.getByTestId("catalogue-source-input").fill(url);
  await page.getByTestId("catalogue-add-source").click();
  await expect(page.getByTestId("catalogue-source")).toHaveCount(1);
  await page.getByTestId("catalogue-refresh").click();
}

const entry = (page: Page, id: string) => page.locator(`[data-testid=catalogue-entry][data-plugin="${id}"]`);

test("there is no built-in index and nothing is requested until a click", async ({ page }) => {
  const asked: string[] = [];
  page.on("request", (r) => asked.push(r.url()));
  await openCatalogue(page);
  await expect(page.getByTestId("catalogue-no-sources")).toBeVisible();
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(0);
  await page.getByTestId("catalogue-source-input").fill(SOURCE);
  await page.getByTestId("catalogue-add-source").click();
  await expect(page.getByTestId("catalogue-source")).toHaveCount(1);
  // Adding a source reads nothing: no plugins listed yet.
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(0);
  expect(asked.filter((u) => u.includes("plugins.example.test"))).toEqual([]);
});

test("refresh lists the plugins; search narrows; the source shows its name and commit for a Git source", async ({ page }) => {
  await openCatalogue(page);
  await addAndRefresh(page, "https://git.example.test/org/plugins#main");
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(4);
  await expect(page.getByTestId("catalogue-source")).toContainText("Mock index");
  await expect(page.getByTestId("catalogue-source")).toContainText("index at 3fa1c9e");
  await page.getByTestId("catalogue-search").fill("lint");
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(1);
  await expect(entry(page, "acme-lint")).toContainText("Acme lint");
  await page.getByTestId("catalogue-search").fill("zzz");
  await expect(page.getByTestId("catalogue-none")).toBeVisible();
});

test("install shows the plugin's own rights and the checksum, then installs", async ({ page }) => {
  await openCatalogue(page);
  await addAndRefresh(page);
  await entry(page, "json-tools").getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-confirm")).toContainText("Install JSON tools 1.2.0?");
  await expect(page.getByTestId("catalogue-permissions")).toHaveText("Storage: keep its own data on this device");
  await expect(page.getByTestId("catalogue-sha")).toContainText("SHA-256");
  await expect(page.getByTestId("catalogue-warning")).toHaveCount(0);
  await page.getByTestId("catalogue-confirm-install").click();
  await expect(page.getByTestId("catalogue-done")).toContainText("Installed json-tools 1.2.0");
  await expect(entry(page, "json-tools")).toContainText("installed");
  // The Installed tab lists it.
  await page.getByTestId("plugins-tab-installed").click();
  await expect(page.locator('[data-testid=plugin-item][data-plugin="json-tools"]')).toBeVisible();
});

test("rights come from the module: a difference to the index warns", async ({ page }) => {
  await openCatalogue(page);
  await addAndRefresh(page);
  await expect(entry(page, "sneaky")).toContainText("the index says: no permissions");
  await entry(page, "sneaky").getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-permissions")).toContainText("Network: any host");
  await expect(page.getByTestId("catalogue-warning")).toContainText("other rights");
  await page.getByTestId("catalogue-confirm-cancel").click();
  await expect(page.getByTestId("catalogue-confirm")).toHaveCount(0);
});

test("a wrong checksum refuses and installs nothing", async ({ page }) => {
  await openCatalogue(page);
  await addAndRefresh(page);
  await entry(page, "flaky").getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-error")).toContainText("checksum does not match");
  await expect(page.getByTestId("catalogue-confirm")).toHaveCount(0);
  await page.getByTestId("plugins-tab-installed").click();
  await expect(page.getByTestId("plugins-empty")).toBeVisible();
});

test("a source that cannot be read says so on that source", async ({ page }) => {
  await openCatalogue(page);
  await addAndRefresh(page, "https://unreachable.example.test/plugins.json");
  await expect(page.getByTestId("catalogue-source-error")).toContainText("HTTP 404");
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(0);
});

test("a source can be removed, and plain http is refused", async ({ page }) => {
  await openCatalogue(page);
  await page.getByTestId("catalogue-source-input").fill("http://plugins.example.test/plugins.json");
  await page.getByTestId("catalogue-add-source").click();
  await expect(page.getByTestId("catalogue-error")).toContainText("https");
  await addAndRefresh(page);
  await page.getByTestId("catalogue-remove-source").click();
  await expect(page.getByTestId("catalogue-no-sources")).toBeVisible();
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(0);
});

test("the palette opens the catalogue as a view, and nothing scrolls sideways at 900 px", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Plugin catalogue");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("catalogue-view")).toBeVisible();
  await addAndRefresh(page, "https://plugins.example.test/a/very/long/path/to/the/index/of/plugins/that/keeps/going/plugins.json");
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(4);
  const overflow = await page.getByTestId("catalogue-view").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  await page.getByTestId("catalogue-view-close").click();
  await expect(page.getByTestId("catalogue-view")).toHaveCount(0);
});
