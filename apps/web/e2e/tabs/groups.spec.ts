// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Tab groups: create from the tab menu, rename and recolour in the popover, collapse, drag,
// close, the overflow list and the keyboard path. Mock backend only.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const tabs = (page: Page) => page.getByTestId("tab");
const tab = (page: Page, text: string) => tabs(page).filter({ hasText: text });
const chips = (page: Page) => page.getByTestId("group-chip");
const strip = (page: Page) => page.getByRole("tablist", { name: "Open requests" });
const shown = (page: Page) => page.locator('[data-testid="tab"][aria-selected="true"]');
const menuItem = (page: Page, name: string | RegExp, exact = false) => page.getByRole("menuitem", { name, exact });

/** Opens the demo collection with `count` tabs: Health, then "Request number 2" … */
async function open(page: Page, count = 5, width = 1200) {
  await page.setViewportSize({ width, height: 700 });
  await page.goto("/?backend=mock");
  await page.evaluate(() => localStorage.clear());
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  for (let i = 2; i <= count; i++) {
    await page.keyboard.press(`${MOD}+n`);
    await page.keyboard.press("Enter");
    await page.keyboard.type(`Request number ${i}`);
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("new-dialog")).toHaveCount(0);
  }
  // With many tabs some sit behind "N more".
  if (count <= 5 && width >= 1000) await expect(tabs(page)).toHaveCount(count);
  else await expect(tabs(page).first()).toBeVisible();
}

/** Groups the tab through its context menu and names the group. */
async function group(page: Page, text: string, name: string) {
  await tab(page, text).click({ button: "right" });
  await menuItem(page, "Add tab to new group").click();
  await expect(page.getByTestId("group-popover")).toBeVisible();
  await expect(page.getByTestId("group-name")).toBeFocused();
  await page.keyboard.type(name);
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("group-popover")).toHaveCount(0);
}

const noScrollbar = async (page: Page) =>
  expect(await strip(page).evaluate((el) => el.scrollWidth <= el.clientWidth)).toBe(true);

test("add a tab to a new group: the name popover opens at once, chip and underline appear", async ({ page }) => {
  await open(page);
  await expect(chips(page)).toHaveCount(0);
  await group(page, "Request number 2", "Orders");
  await expect(chips(page)).toHaveCount(1);
  await expect(chips(page)).toContainText("Orders");
  await expect(chips(page)).toHaveAttribute("aria-expanded", "true");
  await expect(tab(page, "Request number 2")).toHaveAttribute("data-group", /.+/);
  await expect(tab(page, "Health")).not.toHaveAttribute("data-group", /.+/);
  // The chip sits right before its tab.
  const order = await strip(page).locator('[data-testid="group-chip"], [data-testid="tab"]').evaluateAll((els) =>
    els.map((e) => (e.getAttribute("data-testid") === "group-chip" ? "chip" : (e.getAttribute("aria-label") ?? "").slice(0, 16))),
  );
  expect(order.indexOf("chip")).toBe(order.indexOf("Request number 2") - 1);
});

test("rename and colour from the chip menu", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Orders");
  const before = await chips(page).getAttribute("style");
  await chips(page).click({ button: "right" });
  await expect(page.getByRole("menu", { name: "Group actions" }).getByRole("menuitem")).toHaveText([
    "Rename…",
    "Colour",
    "New tab in group",
    "Add tab",
    "Add all ungrouped tabs",
    "Collapse group",
    "Ungroup",
    "Close group",
  ]);
  await menuItem(page, "Rename…").click();
  await page.getByTestId("group-name").fill("Billing");
  await page.getByTestId("group-color").nth(4).click();
  await expect(chips(page)).toContainText("Billing");
  await expect(chips(page)).toHaveAttribute("style", /--rt-lane-5/);
  expect(before).not.toBe(await chips(page).getAttribute("style"));
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("group-popover")).toHaveCount(0);
  await expect(chips(page)).toBeFocused();
});

test("click the chip: collapse to a count, click again to expand", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Orders");
  await tab(page, "Request number 3").click({ button: "right" });
  await menuItem(page, "Add tab to group").hover();
  await page.getByRole("menuitem", { name: "Orders" }).click();
  await expect(page.locator('[data-testid="tab"][data-group]')).toHaveCount(2);
  // Show a tab outside the group, then collapse.
  await tab(page, "Health").click();
  await chips(page).click();
  await expect(chips(page)).toHaveAttribute("data-collapsed", "true");
  await expect(page.getByTestId("group-count")).toHaveText("2");
  await expect(tabs(page)).toHaveCount(3);
  await chips(page).click();
  await expect(tabs(page)).toHaveCount(5);
  await expect(page.getByTestId("group-count")).toHaveCount(0);
  await noScrollbar(page);
});

test("collapsing the group of the shown tab shows another tab; choosing a tab expands its group", async ({ page }) => {
  await open(page);
  await group(page, "Request number 5", "Last");
  await expect(shown(page)).toContainText("Request number 5");
  await chips(page).click();
  await expect(chips(page)).toHaveAttribute("data-collapsed", "true");
  await expect(shown(page)).not.toContainText("Request number 5");
  // Mod+Tab style stepping skips the collapsed group.
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press("Control+Tab");
    await expect(shown(page)).not.toContainText("Request number 5");
  }
  // Mod+9 is the last visible tab.
  await page.keyboard.press(`${MOD}+9`);
  await expect(shown(page)).toContainText("Request number 4");
  // Go to tab… lists the hidden tab; choosing it expands the group.
  await page.keyboard.press(`${MOD}+p`);
  await page.getByTestId("tab-list-filter").fill("number 5");
  await page.keyboard.press("Enter");
  await expect(shown(page)).toContainText("Request number 5");
  await expect(chips(page)).toHaveAttribute("data-collapsed", "false");
});

test("remove from group, ungroup, and closing the last tab removes the group", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "A");
  await tab(page, "Request number 2").click({ button: "right" });
  await menuItem(page, "Remove from group").click();
  await expect(chips(page)).toHaveCount(0);
  await group(page, "Request number 3", "B");
  await chips(page).click({ button: "right" });
  await menuItem(page, "Ungroup", true).click();
  await expect(chips(page)).toHaveCount(0);
  await expect(tabs(page)).toHaveCount(5);
  await group(page, "Request number 4", "C");
  await tab(page, "Request number 4").getByRole("button", { name: /Close/ }).click({ force: true });
  await expect(chips(page)).toHaveCount(0);
});

test("close group closes its tabs", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Gone");
  await tab(page, "Request number 3").click({ button: "right" });
  await menuItem(page, "Add tab to group").hover();
  await page.getByRole("menuitem", { name: "Gone" }).click();
  await chips(page).click({ button: "right" });
  await menuItem(page, "Close group").click();
  await expect(chips(page)).toHaveCount(0);
  await expect(tabs(page)).toHaveCount(3);
  await expect(tab(page, "Request number 2")).toHaveCount(0);
});

test("a new tab in the group joins it", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Flow");
  await chips(page).click({ button: "right" });
  await menuItem(page, "New tab in group").click();
  await page.keyboard.press("Enter");
  await page.keyboard.type("Joined request");
  await page.keyboard.press("Enter");
  await expect(tab(page, "Joined request")).toHaveAttribute("data-group", /.+/);
  const order = await tabs(page).evaluateAll((els) => els.map((e) => [e.getAttribute("aria-label")?.slice(0, 14), e.getAttribute("data-group")]));
  const at = order.findIndex(([t]) => t === "Joined request");
  expect(order[at - 1][1]).toBe(order[at][1]);
});

test("drag a tab onto a chip, out of a group, and a whole group by its chip", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Drag");
  // Into the group: drop on the chip.
  await tab(page, "Request number 5").dragTo(chips(page));
  await expect(tab(page, "Request number 5")).toHaveAttribute("data-group", /.+/);
  // Out of the group: drop on an ungrouped tab.
  await tab(page, "Request number 5").dragTo(tab(page, "Request number 4"));
  await expect(tab(page, "Request number 5")).not.toHaveAttribute("data-group", /.+/);
  // Reorder plain tabs.
  await tab(page, "Request number 4").dragTo(tab(page, "Health"), { targetPosition: { x: 4, y: 10 } });
  await expect(tabs(page).first()).toContainText("Request number 4");
  // Move the group: drag the chip onto the last tab's right half.
  const last = tabs(page).last();
  const box = (await last.boundingBox())!;
  await chips(page).dragTo(last, { targetPosition: { x: box.width - 6, y: 10 } });
  const kinds = await strip(page).locator('[data-testid="group-chip"], [data-testid="tab"]').evaluateAll((els) =>
    els.map((e) => (e.getAttribute("data-testid") === "group-chip" ? "chip" : "tab")),
  );
  expect(kinds.at(-2)).toBe("chip");
  expect(kinds.at(-1)).toBe("tab");
});

test("keyboard only: menu key, move with Alt+arrows, chip menu, collapse with Enter", async ({ page }) => {
  await open(page);
  await tab(page, "Request number 3").focus();
  await page.keyboard.press("Shift+F10");
  await expect(page.getByRole("menu", { name: "Tab actions" })).toBeVisible();
  await page.keyboard.press("a");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Keys");
  await page.keyboard.press("Enter");
  await expect(chips(page)).toContainText("Keys");
  // The chip got focus back; Enter collapses, Enter expands.
  await expect(chips(page)).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(chips(page)).toHaveAttribute("data-collapsed", "true");
  await page.keyboard.press("Enter");
  await expect(chips(page)).toHaveAttribute("data-collapsed", "false");
  // Alt+Arrow moves the chip (the group) one block to the right.
  await page.keyboard.press("Alt+ArrowRight");
  const kinds = async () =>
    strip(page).locator('[data-testid="group-chip"], [data-testid="tab"]').evaluateAll((els) =>
      els.map((e) => (e.getAttribute("data-testid") === "group-chip" ? "chip" : (e.textContent ?? "").trim().slice(0, 16))),
    );
  expect((await kinds()).indexOf("chip")).toBe(3);
  // Alt+Arrow on the grouped tab at the group's edge leaves the group.
  await tab(page, "Request number 3").focus();
  await page.keyboard.press("Alt+ArrowRight");
  await expect(tab(page, "Request number 3")).not.toHaveAttribute("data-group", /.+/);
  await expect(chips(page)).toHaveCount(0);
  // Rename with F2 needs a group: make one with the palette.
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Group tabs: new group");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("group-popover")).toBeVisible();
  await page.keyboard.type("Pal");
  await page.keyboard.press("Enter");
  await expect(chips(page)).toContainText("Pal");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Group tabs: collapse");
  await page.keyboard.press("Enter");
  await expect(chips(page)).toHaveAttribute("data-collapsed", "true");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Group tabs: close group");
  await page.keyboard.press("Enter");
  await expect(chips(page)).toHaveCount(0);
});

test("groups survive a reload (per device)", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Stays");
  const stored = await page.evaluate(() => localStorage.getItem("rt.shell.tab-groups"));
  expect(stored).toContain("Stays");
});

test("the overflow list shows groups as coloured sections", async ({ page }) => {
  await open(page, 12, 1000);
  await group(page, "Request number 3", "Late");
  await group(page, "Request number 2", "Early");
  await page.getByTestId("tab-more").click();
  const sections = page.getByTestId("tab-list-group");
  await expect(sections).toHaveCount(2);
  await expect(sections.first()).toContainText("Early");
  await expect(sections.last()).toContainText("Late");
  await expect(page.getByTestId("tab-list-row")).toHaveCount(12);
  await expect(sections.first().locator(".swatch")).toBeVisible();
});

for (const width of [800, 1000]) {
  test(`narrow strip ${width}px: no scrollbar with groups, active tab stays visible`, async ({ page }) => {
    await open(page, 10, width);
    await group(page, "Request number 10", "One");
    await group(page, "Health", "A much longer group name");
    await noScrollbar(page);
    await expect(shown(page)).toHaveCount(1);
    await chips(page).first().click();
    await noScrollbar(page);
    const box = await strip(page).boundingBox();
    for (const t of await tabs(page).all()) {
      const b = (await t.boundingBox())!;
      expect(b.x + b.width).toBeLessThanOrEqual(box!.x + box!.width + 1);
    }
  });
}

for (const scheme of ["light", "dark"] as const) {
  test(`screenshot grouped strip ${scheme}`, async ({ page }, info) => {
    await page.emulateMedia({ colorScheme: scheme });
    await open(page, 5, 1300);
    await group(page, "Request number 2", "Orders");
    await tab(page, "Request number 3").click({ button: "right" });
    await menuItem(page, "Add tab to group").hover();
    await page.getByRole("menuitem", { name: "Orders" }).click();
    await group(page, "Request number 4", "Billing");
    await group(page, "Request number 5", "");
    await page.keyboard.press("Escape");
    await chips(page).nth(1).click();
    await tab(page, "Health").click();
    await page.screenshot({ path: info.outputPath(`tab-groups-${scheme}.png`), clip: { x: 0, y: 0, width: 1300, height: 110 } });
    await chips(page).first().click({ button: "right" });
    await page.screenshot({ path: info.outputPath(`tab-groups-${scheme}-menu.png`), clip: { x: 0, y: 0, width: 1300, height: 330 } });
  });
}

test("chip menu: Add tab moves a chosen tab into the group, Add all moves the rest", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Orders");
  await chips(page).click({ button: "right" });
  await menuItem(page, "Add tab").click();
  await menuItem(page, /Request number 4/).click();
  await expect(tab(page, "Request number 4")).toHaveAttribute("data-group", /.+/);
  await expect(tab(page, "Request number 5")).not.toHaveAttribute("data-group", /.+/);
  await chips(page).click({ button: "right" });
  await menuItem(page, "Add all ungrouped tabs").click();
  for (const t of ["Health", "Request number 3", "Request number 5"]) await expect(tab(page, t)).toHaveAttribute("data-group", /.+/);
  await chips(page).click({ button: "right" });
  await expect(menuItem(page, "Add tab")).toBeDisabled();
});

test("drag a tab onto a grouped tab: it joins that group", async ({ page }) => {
  await open(page);
  await group(page, "Request number 2", "Drag");
  await tab(page, "Request number 4").dragTo(tab(page, "Request number 2"));
  await expect(tab(page, "Request number 4")).toHaveAttribute("data-group", /.+/);
  await expect(chips(page)).toHaveCount(1);
});
