// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D (gate item 8): the 3-way view of a conflicted file. Original, mine and
// theirs side by side, one choice per conflicting place, an editable result, resolved with
// GitApi.resolveText.

import { expect, test } from "@playwright/test";

import type { W } from "../git-wizards/helpers";
import { conflictedMerge, reopenPanel } from "./helpers";

test("resolves a conflict line by line and the merge can be finished", async ({ page }) => {
  await conflictedMerge(page);
  await reopenPanel(page);
  await page.locator('[data-testid="git-file"][data-path$="health.yml"]').click();
  await page.getByTestId("git-resolve-by-hand").click();

  const view = page.getByTestId("text-merge");
  await expect(view).toBeVisible();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "text-merge");
  await expect(view.getByTestId("text-merge-count")).toContainText("your decision");
  const block = view.getByTestId("text-merge-block").first();
  await expect(block.getByTestId("text-merge-mine")).toContainText("example.test/ours");
  await expect(block.getByTestId("text-merge-their")).toContainText("example.test/theirs");
  await expect(view.getByTestId("text-merge-result")).toHaveValue(/example\.test\/ours/);
  await block.getByTestId("text-merge-theirs").click();
  await expect(view.getByTestId("text-merge-result")).toHaveValue(/example\.test\/theirs/);
  await expect(view.getByTestId("text-merge-result")).not.toHaveValue(/example\.test\/ours/);

  await view.getByTestId("text-merge-resolve").click();
  await expect(view).toBeHidden();
  const url = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.getRequest("health.yml")).url);
  expect(url).toBe("https://example.test/theirs");
  const status = await page.evaluate(() => (window as unknown as W).__rt.backend.git.status());
  expect(status.files.some((f) => f.state === "conflicted")).toBe(false);
});

test("says why it cannot resolve while there are unsaved changes", async ({ page }) => {
  await conflictedMerge(page);
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).first().click();
  await page.getByTestId("method-select").selectOption("POST");
  await expect(page.getByTestId("save")).toBeEnabled();
  await page.getByTestId("view-git").click();
  await page.locator('[data-testid="git-file"][data-path$="health.yml"]').click();
  await page.getByTestId("git-resolve-by-hand").click();
  await expect(page.getByTestId("text-merge")).toContainText("Save your changes first");
  await expect(page.getByTestId("text-merge-resolve")).toBeDisabled();
});
