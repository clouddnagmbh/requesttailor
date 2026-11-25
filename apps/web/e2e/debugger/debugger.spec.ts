// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand Y: the script debugger against the mock backend (`?backend=mock`). The mock
// steps through the four lines of the "List users" before-request script: breakpoint, steps,
// variables, an expression, abort, and masking of the secret token.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openDebugger(page: Page) {
  await page.goto("/?backend=mock&debugger=1");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Debug scripts");
  await palette.getByRole("option").filter({ hasText: "Debug scripts" }).click();
  await expect(page.getByTestId("debugger-panel")).toBeVisible();
}

test("stops at a breakpoint, shows variables, answers an expression and steps", async ({ page }) => {
  await openDebugger(page);
  await expect(page.getByTestId("debugger-line")).toHaveCount(4);
  await page.getByTestId("debugger-gutter").nth(2).click();
  await page.getByTestId("debugger-start").click();
  await expect(page.getByTestId("debugger-status")).toContainText("Stopped at line 3 (breakpoint)");
  await expect(page.getByTestId("debugger-line").nth(2)).toHaveClass(/current/);
  await expect(page.getByTestId("debugger-var").filter({ hasText: "who" })).toContainText('"debugger"');
  await page.getByTestId("debugger-expression").fill("n * 7");
  await page.getByTestId("debugger-expression").press("Enter");
  await expect(page.getByTestId("debugger-answer")).toContainText("= 42");
  await page.getByTestId("debugger-step-over").click();
  await expect(page.getByTestId("debugger-status")).toContainText("Stopped at line 4 (step)");
  await page.getByTestId("debugger-continue").click();
  await expect(page.getByTestId("debugger-status")).toHaveText("Finished");
  await expect(page.getByTestId("debugger-output")).toContainText("token ***");
});

test("a secret is masked in what the debugger shows, and abort ends the session", async ({ page }) => {
  await openDebugger(page);
  await page.getByTestId("debugger-start").click();
  await expect(page.getByTestId("debugger-status")).toContainText("Stopped at line 1 (entry)");
  await page.getByTestId("debugger-expression").fill("token");
  await page.getByTestId("debugger-expression").press("Enter");
  await expect(page.getByTestId("debugger-answer")).toContainText("***");
  await page.getByTestId("debugger-abort").click();
  await expect(page.getByTestId("debugger-status")).toHaveText("Aborted");
  await expect(page.getByTestId("debugger-continue")).toBeDisabled();
});
