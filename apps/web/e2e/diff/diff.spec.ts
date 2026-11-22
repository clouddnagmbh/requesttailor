// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand A: the semantic diff view against the mock backend (`?backend=mock`): saved
// changes per field, the line-diff toggle, a commit from the log, and the palette entry.

import { expect, test, type Page } from "@playwright/test";

async function openGit(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const git = (window as unknown as { __rt: { backend: { git: { setIdentity(i: object): Promise<void> } } } }).__rt.backend.git;
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
}

async function editHealth(page: Page) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("method-select").selectOption("POST");
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();
}

test("saved changes are shown per field, with a line-diff toggle", async ({ page }) => {
  await openGit(page);
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  await editHealth(page);

  await page.getByTestId("git-semantic-diff").click();
  const view = page.getByTestId("semantic-diff-view");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("sd-summary")).toHaveText("1 modified");
  const change = page.getByTestId("sd-change");
  await expect(change).toHaveCount(1);
  await expect(change.getByTestId("sd-name")).toHaveText("Health");
  await expect(change.getByTestId("sd-badge")).toHaveText("Modified");
  const method = change.getByTestId("sd-field");
  await expect(method).toHaveCount(1);
  await expect(method.getByTestId("sd-path")).toHaveText("method");
  await expect(method.getByTestId("sd-old")).toHaveText("GET");
  await expect(method.getByTestId("sd-new")).toHaveText("POST");

  // The card does not overflow the view.
  const box = await change.boundingBox();
  const outer = await view.boundingBox();
  expect(box && outer && box.x >= outer.x && box.x + box.width <= outer.x + outer.width + 1).toBe(true);

  await page.getByTestId("sd-mode-lines").click();
  await expect(page.getByTestId("sd-lines")).toBeVisible();
  await expect(page.getByTestId("line-diff")).toHaveCount(1);
  await expect(page.getByTestId("line-diff").locator(".line.added")).toContainText("POST");
  await expect(page.getByTestId("line-diff").locator(".line.removed")).toContainText("GET");
  await page.getByTestId("sd-mode-fields").click();
  await expect(change).toHaveCount(1);
  await page.screenshot({ path: test.info().outputPath("semantic-diff.png") });
});

test("a commit from the log and the palette entry open the diff", async ({ page }) => {
  await openGit(page);
  await page.getByTestId("view-git").click();
  await editHealth(page);
  await page.getByTestId("git-file-check").first().waitFor();
  await page.getByTestId("git-message").fill("Health is a POST");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();

  await page.getByTestId("git-commit-changes").first().click();
  await expect(page.getByTestId("sd-title")).toContainText("against its parent");
  await expect(page.getByTestId("sd-new")).toHaveText("POST");
  await page.getByTestId("git-view-close").click();
  await expect(page.getByTestId("git-view")).toHaveCount(0);

  await page.keyboard.press("ControlOrMeta+K");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("by field");
  await palette.getByRole("option", { name: /Changes: saved changes by field/ }).click();
  await expect(page.getByTestId("semantic-diff-view")).toHaveAttribute("data-scope", "working");
  await expect(page.getByTestId("sd-empty")).toBeVisible();
});
