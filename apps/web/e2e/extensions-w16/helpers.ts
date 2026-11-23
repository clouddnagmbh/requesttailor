// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { expect, type Page } from "@playwright/test";

export const MOD = "ControlOrMeta";
export const URL = "/?backend=mock&extensions=demo16";

export async function openCollection(page: Page, url = URL) {
  await page.goto(url);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

export async function openRequest(page: Page, name = "List users") {
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
}

export async function runCommand(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill(label);
  await palette.getByRole("option").first().click();
}

/** The lines the demo extension logged (`window.__demo16`). */
export const demoLog = (page: Page) => page.evaluate(() => (window as unknown as { __demo16?: string[] }).__demo16 ?? []);

/** The text of a CodeMirror editor. */
export const editorText = (page: Page, testid: string) => page.getByTestId(testid).locator(".cm-content").innerText();

/** No element of the page is wider than the window (no horizontal overflow). */
export async function expectNoOverflow(page: Page) {
  const over = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(over).toBeLessThanOrEqual(0);
}
