// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand C: hints, node chips, extension dialogs and the tab openers, with the demo
// extension of the dev server (`?backend=mock&extensions=demo`). Mock backend only.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const URL = "/?backend=mock&extensions=demo";

async function openCollection(page: Page) {
  await page.goto(URL);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const hint = (page: Page) => page.getByTestId("extension-hint");

async function runCommand(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill(label);
  await palette.getByRole("option").first().click();
}

test("a hint shows above the request, can be dismissed and stays away after a reload", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(hint(page)).toHaveCount(1);
  await expect(hint(page)).toContainText("Demo hint");
  const box = await hint(page).boundingBox();
  const urlbar = await page.getByTestId("method-select").boundingBox();
  expect(box!.y + box!.height).toBeLessThanOrEqual(urlbar!.y);

  // It is the same hint on another request: one hint per place.
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await expect(hint(page)).toHaveCount(1);

  await hint(page).getByRole("button", { name: "Dismiss hint" }).click();
  await expect(hint(page)).toHaveCount(0);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(hint(page)).toHaveCount(0);

  await page.reload();
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
  await expect(hint(page)).toHaveCount(0);
});

test("a hint button opens the request tab of the extension", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await hint(page).getByTestId("hint-action").click();
  await expect(page.getByTestId("demo-tab")).toContainText("Demo request tab for");
  // The hint stays (the button does not dismiss it).
  await expect(hint(page)).toHaveCount(1);
});

test("a command opens the collection tab of the extension", async ({ page }) => {
  await openCollection(page);
  await runCommand(page, "Demo: open overview");
  await expect(page.getByTestId("demo-tab")).toHaveText("Demo collection tab");
});

test("an extension dialog traps focus, closes with Escape and gives focus back", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await runCommand(page, "Demo: show dialog");
  const dialog = page.getByRole("dialog", { name: "Demo dialog" });
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("demo-input")).toBeFocused();
  // Tab stays inside, both ways.
  for (let i = 0; i < 6; i++) {
    await page.keyboard.press("Tab");
    expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
  }
  await page.keyboard.press("Shift+Tab");
  expect(await dialog.evaluate((el) => el.contains(document.activeElement))).toBe(true);
  // Width as asked for (420), title bar and close button.
  expect((await dialog.boundingBox())!.width).toBe(420);
  await expect(dialog.getByRole("button", { name: "Close" })).toBeVisible();

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __demoResult?: unknown }).__demoResult)).toBeUndefined();
});

test("an extension dialog resolves with what it closes with", async ({ page }) => {
  await openCollection(page);
  await runCommand(page, "Demo: show dialog");
  await page.getByTestId("demo-input").fill("Ada");
  await page.getByTestId("demo-done").click();
  await expect(page.getByRole("dialog", { name: "Demo dialog" })).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as { __demoResult?: unknown }).__demoResult)).toBe("Ada");
});

for (const width of [900, 1440]) {
  test(`hint and dialog fit at ${width} px`, async ({ page }) => {
    await page.setViewportSize({ width, height: 800 });
    await openCollection(page);
    await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
    const h = (await hint(page).boundingBox())!;
    expect(h.x + h.width).toBeLessThanOrEqual(width);
    await runCommand(page, "Demo: show dialog");
    const d = (await page.getByTestId("extension-dialog").boundingBox())!;
    expect(d.x).toBeGreaterThanOrEqual(0);
    expect(d.x + d.width).toBeLessThanOrEqual(width);
    expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
  });
}

// The chip needs the sidebar of strand A (it places `NodeChip` and the context-menu entries).
test.fixme("an inline node action shows as a chip on a folder row and opens the collection tab", async ({ page }) => {
  await openCollection(page);
  const row = page.getByTestId("tree-folder").filter({ hasText: "users" });
  await row.getByTestId("node-chip").click();
  await expect(page.getByTestId("demo-tab")).toHaveText("Demo collection tab");
});
