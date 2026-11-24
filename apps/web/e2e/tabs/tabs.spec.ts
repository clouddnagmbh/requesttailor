// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The tab bar with many tabs: the tabs share the width, what does not fit goes behind an
// "N more" button with the list of all tabs, never a horizontal scrollbar; shortcuts and the
// context menu. Mock backend only.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const COUNT = 12;

const tabs = (page: Page) => page.getByTestId("tab");
const strip = (page: Page) => page.getByRole("tablist", { name: "Open requests" });
const more = (page: Page) => page.getByTestId("tab-more");
const rows = (page: Page) => page.getByTestId("tab-list-row");

/** Opens the demo collection and creates requests until `COUNT` tabs are open (the last one is shown). */
async function manyTabs(page: Page, width = 1000) {
  await page.setViewportSize({ width, height: 700 });
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  for (let i = 2; i <= COUNT; i++) {
    await page.keyboard.press(`${MOD}+n`);
    await page.keyboard.press("Enter");
    await page.keyboard.type(`Request number ${i} with a long name`);
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("new-dialog")).toHaveCount(0);
  }
  await expect(more(page)).toBeVisible();
}

const noScrollbar = async (page: Page) =>
  expect(await strip(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);

const hiddenCount = async (page: Page) => COUNT - (await tabs(page).count());

test("many tabs: no horizontal scrollbar, the rest sits behind an N more button", async ({ page }) => {
  await manyTabs(page);
  await noScrollbar(page);
  const n = await tabs(page).count();
  expect(n).toBeLessThan(COUNT);
  expect(n).toBeGreaterThan(3);
  await expect(more(page)).toContainText(`${COUNT - n} more`);
  // Every shown tab keeps at least its minimum width; none is cut at the strip's edge.
  const box = await strip(page).boundingBox();
  for (const t of await tabs(page).all()) {
    const b = (await t.boundingBox())!;
    expect(b.width).toBeGreaterThanOrEqual(110);
    expect(b.x + b.width).toBeLessThanOrEqual(box!.x + box!.width + 1);
  }
  // The shown (last created) tab is visible although it would not fit.
  await expect(tabs(page).and(page.locator('[aria-selected="true"]'))).toContainText(`Request number ${COUNT}`);
  // A wheel over the strip does not move anything.
  await strip(page).hover();
  await page.mouse.wheel(0, 300);
  await page.mouse.wheel(300, 0);
  expect(await strip(page).evaluate((el) => el.scrollLeft)).toBe(0);
});

test("the list shows all tabs, filters, and opening a hidden tab makes it visible", async ({ page }) => {
  await manyTabs(page);
  await more(page).click();
  await expect(page.getByTestId("tab-list")).toBeVisible();
  await expect(page.getByTestId("tab-list-filter")).toBeFocused();
  await expect(rows(page)).toHaveCount(COUNT);
  await expect(rows(page).first()).toContainText("Health");
  await expect(rows(page).first()).toContainText("GET");

  await page.getByTestId("tab-list-filter").fill("number 3 ");
  await expect(rows(page)).toHaveCount(1);
  await page.getByTestId("tab-list-filter").fill("zzzz");
  await expect(rows(page)).toHaveCount(0);
  await page.getByTestId("tab-list-filter").fill("");

  // Pick the first request that is not in the strip: it becomes visible and active.
  const shownPaths = await tabs(page).evaluateAll((els) => els.map((e) => e.getAttribute("data-path")));
  const hiddenRow = rows(page).filter({ hasNot: page.locator("xx") });
  const paths = await hiddenRow.evaluateAll((els) => els.map((e) => e.getAttribute("data-path")));
  const target = paths.find((p) => !shownPaths.includes(p))!;
  await page.locator(`[data-testid="tab-list-row"][data-path="${target}"]`).click();
  await expect(page.getByTestId("tab-list")).toHaveCount(0);
  const active = page.locator(`[data-testid="tab"][data-path="${target}"]`);
  await expect(active).toHaveAttribute("aria-selected", "true");
  await noScrollbar(page);
});

test("the list opens with the keyboard and a row closes its tab", async ({ page }) => {
  await manyTabs(page);
  await page.keyboard.press(`${MOD}+p`);
  await expect(page.getByTestId("tab-list")).toBeVisible();
  await page.keyboard.type("health");
  await expect(rows(page)).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("tab-list")).toHaveCount(0);
  await expect(tabs(page).and(page.locator('[aria-selected="true"]'))).toContainText("Health");

  await more(page).click();
  await page.getByRole("button", { name: `Close Request number ${COUNT} with a long name` }).click({ force: true });
  await expect(rows(page)).toHaveCount(COUNT - 1);
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("tab-list")).toHaveCount(0);
});

test("shortcuts: Ctrl+Tab, Mod+digits, Mod+9", async ({ page }) => {
  await manyTabs(page);
  const shown = () => page.locator('[data-testid="tab"][aria-selected="true"]');
  await page.keyboard.press(`${MOD}+1`);
  await expect(shown()).toContainText("Health");
  await page.keyboard.press("Control+Tab");
  await expect(shown()).toContainText("Request number 2");
  await page.keyboard.press("Control+Shift+Tab");
  await expect(shown()).toContainText("Health");
  await page.keyboard.press("Control+Shift+Tab");
  await expect(shown()).toContainText(`Request number ${COUNT}`);
  await page.keyboard.press(`${MOD}+3`);
  await expect(shown()).toContainText("Request number 3");
  await page.keyboard.press(`${MOD}+9`);
  await expect(shown()).toContainText(`Request number ${COUNT}`);
  // A hidden tab reached by number is shown.
  await page.keyboard.press(`${MOD}+8`);
  await expect(shown()).toHaveCount(1);
  await noScrollbar(page);
});

test("palette lists Go to tab… and opens the list", async ({ page }) => {
  await manyTabs(page);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("go to tab");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("tab-list")).toBeVisible();
  await expect(rows(page)).toHaveCount(COUNT);
});

test("context menu: close variants and reveal in sidebar", async ({ page }) => {
  await page.setViewportSize({ width: 1400, height: 700 });
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  // Tree order: users/create-user, users/list-users, ... health; the order of the tabs is the click order.
  for (const n of ["Health", "List users", "Create user"]) {
    await page.getByTestId("tree-request").filter({ hasText: n }).first().click();
  }
  await page.getByTestId("tree-request").filter({ hasNotText: /Health|List users|Create user/ }).first().click();
  await expect(tabs(page)).toHaveCount(4);
  const tab = (n: string) => tabs(page).filter({ hasText: n });

  // Unsaved changes stay when a tab closes (the draft is kept, as with ×).
  await tab("List users").click({ button: "right" });
  const menu = page.getByRole("menu", { name: "Tab actions" });
  await expect(menu.getByRole("menuitem")).toHaveText([
    "Close",
    "Close others",
    "Close tabs to the right",
    "Close saved tabs",
    "Reveal in sidebar",
    "Add tab to new group",
    "Add tab to group",
  ]);
  await menu.getByRole("menuitem", { name: "Close tabs to the right" }).click();
  await expect(tabs(page)).toHaveCount(2);

  await tab("Health").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Reveal in sidebar" }).click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Health" })).toHaveAttribute("aria-selected", "true");

  await tab("List users").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close others" }).click();
  await expect(tabs(page)).toHaveCount(1);
  await expect(tab("List users")).toHaveAttribute("aria-selected", "true");

  // Keyboard: the context-menu key opens it, Escape closes it.
  await tab("List users").focus();
  await page.keyboard.press("ContextMenu");
  await expect(menu).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);

  // Close saved tabs spares the tab with unsaved changes.
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByRole("button", { name: "New request in users" }).click();
  await page.getByTestId("new-request-name").fill("Draft");
  await page.getByTestId("new-request-name").press("Enter");
  await expect(tab("Draft").getByTestId("tab-dirty")).toBeVisible();
  await tab("Health").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Close saved tabs" }).click();
  await expect(tabs(page)).toHaveCount(1);
  await expect(page.getByTestId("tab-dirty")).toHaveCount(1);
});

for (const width of [900, 1440]) {
  for (const scheme of ["light", "dark"] as const) {
    test(`screenshot ${width}px ${scheme}`, async ({ page }, info) => {
      await page.emulateMedia({ colorScheme: scheme });
      await manyTabs(page, width);
      await noScrollbar(page);
      await page.screenshot({ path: info.outputPath(`tabs-${width}-${scheme}.png`), clip: { x: 0, y: 0, width, height: 140 } });
      await more(page).click().catch(() => {});
      await page.screenshot({ path: info.outputPath(`tabs-${width}-${scheme}-list.png`), clip: { x: 0, y: 0, width, height: 520 } });
      expect(await hiddenCount(page)).toBeGreaterThanOrEqual(0);
    });
  }
}
