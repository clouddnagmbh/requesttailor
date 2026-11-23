// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand A: the semantic diff in the web app with the real core (rt-wasm
// `semanticDiff`) and isomorphic-git on an OPFS folder: saved changes against HEAD, a commit
// against its parent and a branch compare show the same field changes the desktop and
// `rt diff` show.

import { expect, test, type Page } from "@playwright/test";

import { newBranch } from "../git-wizards/helpers";

import { fakePicker, fillFolder, sampleCollection } from "./env";

async function setIdentityWhenAsked(page: Page) {
  const form = page.getByTestId("git-identity");
  await expect(form).toBeVisible();
  await form.getByTestId("git-identity-name").fill("Ada Lovelace");
  await form.getByTestId("git-identity-email").fill("ada@example.com");
  await form.getByTestId("git-identity-save").click();
  await expect(form).toBeHidden();
}

test("saved changes, a commit and a branch compare, per field", async ({ page }) => {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-init").click();
  await page.getByTestId("git-message").fill("Initial collection");
  await page.getByTestId("git-commit").click();
  await setIdentityWhenAsked(page);
  await expect(page.getByTestId("git-clean")).toBeVisible();

  await newBranch(page, "feature");
  await expect(page.getByTestId("git-branch")).toHaveText("feature");

  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("method-select").selectOption("POST");
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();

  // Saved changes against HEAD.
  await page.getByTestId("git-semantic-diff").click();
  const field = page.getByTestId("sd-field");
  await expect(page.getByTestId("sd-summary")).toHaveText("1 modified");
  await expect(field.getByTestId("sd-path")).toHaveText("method");
  await expect(field.getByTestId("sd-old")).toHaveText("GET");
  await expect(field.getByTestId("sd-new")).toHaveText("POST");
  await expect(page.getByTestId("sd-uncovered")).toHaveCount(0);

  // After the commit it is the last commit's diff and a compare with main.
  await page.getByTestId("git-view-close").click();
  await page.getByTestId("git-message").fill("Health is a POST");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  await page.getByTestId("git-commit-changes").first().click();
  await expect(field.getByTestId("sd-new")).toHaveText("POST");
  await page.getByTestId("sd-mode-lines").click();
  await expect(page.getByTestId("line-diff").locator('[data-kind="added"]')).toContainText("method: POST");
  await page.getByTestId("git-view-close").click();

  await page.getByTestId("git-compare-branch").selectOption("main");
  await expect(page.getByTestId("sd-title")).toHaveText("feature → main");
  await expect(field.getByTestId("sd-old")).toHaveText("POST");
  await expect(field.getByTestId("sd-new")).toHaveText("GET");
});
