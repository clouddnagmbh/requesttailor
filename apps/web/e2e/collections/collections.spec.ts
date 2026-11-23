// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 14, strand A (collections in the sidebar), mock backend: "Demo API"
// and "Billing API" in one folder (both have `health.yml`), "Add collection…" adds "Payments
// API". Layout checks measure the sidebar at 900 and 1440 px in both themes and the view bar
// at 200, 260 and 400 px; with RT_SHOTS=<dir> they also save screenshots there.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

const MOD = "ControlOrMeta";
const DEMO = "mock-collection";
const BILLING = "mock-collection/billing";
type W = Window & { __rt: { backend: MockBackend } };

const shots = process.env.RT_SHOTS;
async function shot(page: Page, name: string) {
  if (shots) await page.screenshot({ path: `${shots}/${name}.png` });
}

async function open(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const node = (page: Page, name: string) => page.getByTestId("tree-collection").filter({ hasText: name });
const request = (page: Page, collection: string, name: string) =>
  page.locator(`[data-testid="tree-request"][data-collection="${collection}"]`).filter({ hasText: name });
const tab = (page: Page, collection: string, path: string) =>
  page.locator(`[data-testid="tab"][data-collection="${collection}"][data-path="${path}"]`);

test("every collection of the folder is a node; others are read when expanded", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("tree-collection")).toHaveText([/Demo API/, /Billing API/]);
  await expect(node(page, "Demo API")).toHaveAttribute("aria-expanded", "true");
  await expect(node(page, "Billing API")).toHaveAttribute("aria-expanded", "false");
  await expect(page.locator('[data-collection="mock-collection/billing"][data-testid="tree-request"]')).toHaveCount(0);
  await node(page, "Billing API").click();
  await expect(request(page, BILLING, "List invoices")).toBeVisible();
  await expect(request(page, BILLING, "List invoices")).toHaveAttribute("aria-level", "3");
  // Keyboard: ArrowLeft folds it again.
  await node(page, "Billing API").focus();
  await page.keyboard.press("ArrowLeft");
  await expect(request(page, BILLING, "List invoices")).toHaveCount(0);
});

test("tabs of two collections with the same path; the environment follows the tab", async ({ page }) => {
  await open(page);
  await request(page, DEMO, "Health").click();
  await expect(page.getByTestId("env-active")).toHaveText("Local");
  await node(page, "Billing API").click();
  await request(page, BILLING, "Billing health").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Billing API");
  await expect(tab(page, BILLING, "health.yml")).toHaveAttribute("aria-selected", "true");
  await expect(tab(page, DEMO, "health.yml")).toHaveAttribute("aria-selected", "false");
  await expect(page.getByTestId("env-active")).toHaveText("Staging");
  await expect(page.getByTestId("request-editor")).toContainText("{{baseUrl}}/billing/health");

  await tab(page, DEMO, "health.yml").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await expect(page.getByTestId("env-active")).toHaveText("Local");
  await expect(page.getByTestId("request-editor")).toContainText("{{baseUrl}}/health");
  await expect(page.getByTestId("tab")).toHaveCount(2);
});

test("Mod+S saves the changes of every collection", async ({ page }) => {
  await open(page);
  await page.getByRole("button", { name: "New request in users" }).click();
  await page.getByTestId("new-request-name").fill("Demo draft");
  await page.getByTestId("new-request-name").press("Enter");
  await node(page, "Billing API").click();
  await node(page, "Billing API").hover();
  await page.getByRole("button", { name: "New request in Billing API" }).click();
  await page.getByTestId("new-request-name").fill("Billing draft");
  await page.getByTestId("new-request-name").press("Enter");
  await expect(page.getByTestId("collection-dirty")).toHaveCount(2);
  await page.keyboard.press(`${MOD}+s`);
  await expect(page.getByTestId("collection-dirty")).toHaveCount(0);
  const files = await page.evaluate(() => [...(window as unknown as W).__rt.backend.files.keys()]);
  expect(files).toContain("users/demo-draft.yml");
  expect(files).toContain("billing/billing-draft.yml");
});

test("search finds requests of every collection and opens them in their own", async ({ page }) => {
  await open(page);
  await page.getByTestId("view-search").click();
  await page.getByTestId("search-input").fill("health");
  const groups = page.getByTestId("search-group");
  await expect(groups).toHaveCount(2);
  await groups.filter({ hasText: "Billing health" }).getByTestId("search-hit").first().click();
  await expect(page.getByTestId("collection-name")).toHaveText("Billing API");
  await expect(tab(page, BILLING, "health.yml")).toHaveAttribute("aria-selected", "true");
});

// The mock's history lists Demo API's sends whichever collection is active (mock-history.ts);
// the entry opens in Demo API while Billing API is active.
test("history opens the entry in its collection", async ({ page }) => {
  await open(page);
  await request(page, DEMO, "Health").click();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await node(page, "Billing API").click();
  await request(page, BILLING, "Billing health").click();
  await tab(page, DEMO, "health.yml").getByRole("button", { name: /^Close/ }).click();
  await expect(page.getByTestId("collection-name")).toHaveText("Billing API");
  await page.getByTestId("view-history").click();
  await page.getByTestId("history-entry").first().click();
  await expect(tab(page, DEMO, "health.yml")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
});

test("Add collection… adds a folder; the context menu removes it and runs collections", async ({ page }) => {
  await open(page);
  await page.getByTestId("add-collection").click();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Demo API/, /Billing API/, /Payments API/]);
  await expect(page.getByTestId("collection-name")).toHaveText("Payments API");

  // Keyboard path to the menu: Shift+F10 on the node.
  await node(page, "Billing API").focus();
  await page.keyboard.press("Shift+F10");
  const menu = page.getByRole("menu");
  await expect(menu.getByRole("menuitem")).toContainText(["New…", "New request", "Run collection", "Environments…"]);
  await menu.getByRole("menuitem", { name: "Run collection" }).click();
  await expect(page.getByTestId("runner-title")).toContainText("Billing API");
  await expect(page.getByTestId("collection-name")).toHaveText("Billing API");

  await node(page, "Payments API").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Remove from sidebar" }).click();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Demo API/, /Billing API/]);
  // The folder with two collections says so.
  await node(page, "Demo API").getByTestId("node-menu").click();
  await expect(page.getByRole("menuitem", { name: /Remove “demo” from sidebar/ })).toContainText("2 collections");
});

test("an empty sidebar invites to make or open a collection", async ({ page }) => {
  await page.goto("/?backend=mock");
  const side = page.getByTestId("sidebar");
  await expect(side.getByText("No collections yet")).toBeVisible();
  await expect(side.getByRole("button", { name: "Open folder…" })).toBeVisible();
  await expect(side.getByRole("button", { name: "New…" }).first()).toBeVisible();
});

for (const theme of ["light", "dark"] as const) {
  for (const width of [900, 1440]) {
    test(`sidebar layout at ${width} px, ${theme}`, async ({ page }) => {
      await page.addInitScript((t) => localStorage.setItem("rt.shell.theme", JSON.stringify(t)), theme);
      await page.setViewportSize({ width, height: 760 });
      await open(page);
      await node(page, "Billing API").click();
      await request(page, BILLING, "Billing health").click();
      await request(page, DEMO, "Health").click();
      await node(page, "Billing API").hover();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);

      // No horizontal overflow, rows align: twisties of collections share one x, a child's
      // twisty sits under its parent's icon.
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow).toBeLessThanOrEqual(0);
      const xs = await page
        .getByTestId("tree-collection")
        .evaluateAll((rows) => rows.map((r) => Math.round(r.querySelector(".twisty")!.getBoundingClientRect().x)));
      expect(new Set(xs).size).toBe(1);
      const demoIcon = await node(page, "Demo API").locator(".glyph").boundingBox();
      const folderTwisty = await page.getByTestId("tree-folder").first().locator(".twisty").boundingBox();
      expect(Math.abs(demoIcon!.x - folderTwisty!.x)).toBeLessThanOrEqual(1);
      const rowHeights = await page
        .locator('[role="treeitem"]')
        .evaluateAll((rows) => [...new Set(rows.map((r) => Math.round(r.getBoundingClientRect().height)))]);
      expect(rowHeights).toEqual([28]);
      await shot(page, `sidebar-${width}-${theme}`);

      await node(page, "Billing API").getByTestId("node-menu").click();
      await expect(page.getByRole("menu")).toBeVisible();
      await page.waitForTimeout(300); // the menu fades in
      await shot(page, `menu-${width}-${theme}`);
    });
  }
}

for (const width of [200, 260, 400]) {
  test(`view bar fits without scrolling at ${width} px`, async ({ page }) => {
    await page.addInitScript((w) => localStorage.setItem("rt.shell.sidebarWidth", JSON.stringify(w)), width);
    await page.setViewportSize({ width: 1000, height: 560 });
    await open(page);
    const bar = page.getByRole("tablist", { name: "Sidebar" });
    const fits = await bar.evaluate((el) => el.scrollWidth <= el.clientWidth);
    expect(fits).toBe(true);
    const side = (await page.locator(".side").boundingBox())!;
    const first = (await page.getByTestId("view-requests").boundingBox())!;
    const last = (await page.getByTestId("view-mcp").boundingBox())!;
    // `--rt-side-inset` (12 px) before the first, at least as much after the last.
    expect(Math.round(first.x - side.x)).toBe(12);
    expect(Math.round(side.x + side.width - (last.x + last.width))).toBeGreaterThanOrEqual(12);
    await page.getByTestId("view-git").hover();
    await expect(page.getByRole("tooltip")).toHaveText("Git");
    await shot(page, `views-${width}`);
  });
}
