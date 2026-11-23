// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The UI primitive gallery (`?gallery=1`, dev server only): every primitive renders in both
// themes, menus, tooltips and the dialog work from the keyboard. `RT_GALLERY_SHOTS=<dir>`
// also writes full-page screenshots (light, dark, both, dialog) at 1440 px for design review.

import { expect, test, type Page } from "@playwright/test";

const shots = process.env.RT_GALLERY_SHOTS;

async function shoot(page: Page, name: string, fullPage = true) {
  if (!shots) return;
  await page.mouse.move(0, 0);
  await page.screenshot({ path: `${shots}/${name}.png`, fullPage, animations: "disabled" });
}

test.use({ viewport: { width: 1440, height: 1000 } });

for (const theme of ["light", "dark"] as const) {
  test(`gallery renders every primitive (${theme})`, async ({ page }) => {
    await page.goto(`/?gallery=1&theme=${theme}`);
    await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
    for (const name of ["Foundations", "Buttons", "Menus", "Badges and chips", "Tabs", "Fields", "Choices", "Banners", "Cards", "Feedback", "Dialog", "Icons"]) {
      await expect(page.getByRole("region", { name, exact: true })).toBeVisible();
    }
    await expect(page.getByRole("tablist", { name: "Request parts" })).toBeVisible();
    await expect(page.getByRole("switch", { name: "Follow redirects" })).toBeChecked();
    await page.evaluate(() => document.fonts.ready);
    await shoot(page, `gallery-${theme}`);

    await page.goto(`/?gallery=1&theme=${theme}&menu=1`);
    await expect(page.getByRole("menu", { name: "Branch actions" })).toBeVisible();
    await shoot(page, `gallery-${theme}-menu`, false);
  });
}

test("gallery shows both themes side by side", async ({ page }) => {
  await page.goto("/?gallery=1&theme=both");
  await expect(page.locator('.column[data-theme="light"]')).toBeVisible();
  await expect(page.locator('.column[data-theme="dark"]')).toBeVisible();
  const bg = (sel: string) => page.locator(sel).evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await bg('.column[data-theme="light"]')).not.toBe(await bg('.column[data-theme="dark"]'));
  await shoot(page, "gallery-both");
});

test("menu: keyboard opens, moves, skips disabled items, closes and restores focus", async ({ page }) => {
  await page.goto("/?gallery=1&theme=light");
  const trigger = page.getByRole("button", { name: "Branch actions" });
  await trigger.focus();
  await page.keyboard.press("ArrowDown");
  const menu = page.getByRole("menu", { name: "Branch actions" });
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: /New branch/ })).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(menu.getByRole("menuitem", { name: "Merge into current" })).toBeFocused();
  await page.keyboard.press("ArrowRight");
  const sub = page.getByRole("menu", { name: "Merge into current" });
  await expect(sub.getByRole("menuitem", { name: "feature/pagination" })).toBeFocused();
  await page.keyboard.press("ArrowLeft");
  await expect(sub).toBeHidden();
  await page.keyboard.press("ArrowDown"); // skips the disabled "Rename…"
  await expect(menu.getByRole("menuitemcheckbox", { name: "Show remote branches" })).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
  await expect(trigger).toBeFocused();
});

test("tooltip: shows on keyboard focus, hides on Escape, is not clipped", async ({ page }) => {
  await page.goto("/?gallery=1&theme=light");
  await page.getByRole("button", { name: "Commit 3 files" }).focus();
  await page.keyboard.press("Tab"); // the split button's menu button
  await page.keyboard.press("Tab"); // the Fetch split button
  await page.keyboard.press("Tab");
  await page.keyboard.press("Tab");
  const search = page.getByRole("button", { name: "Search" });
  await expect(search).toBeFocused();
  const tip = page.getByRole("tooltip");
  await expect(tip).toHaveText(/Search/);
  // Rendered at the end of <body>, outside any clipping container.
  expect(await tip.evaluate((el) => el.parentElement === document.body)).toBe(true);
  await page.keyboard.press("Escape");
  await expect(tip).toBeHidden();
});

for (const theme of ["light", "dark"] as const) {
  test(`dialog: focus moves in, Tab stays inside, Escape closes (${theme})`, async ({ page }) => {
    await page.goto(`/?gallery=1&theme=${theme}`);
    const open = page.getByRole("button", { name: "Open dialog" });
    await open.click();
    const dialog = page.getByRole("dialog", { name: "Create branch" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel("Name")).toBeFocused();
    await page.waitForTimeout(250);
    await shoot(page, `gallery-${theme}-dialog`, false);
    for (let i = 0; i < 8; i++) await page.keyboard.press("Tab");
    expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await expect(open).toBeFocused();
  });
}
