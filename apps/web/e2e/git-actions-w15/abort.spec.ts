// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D (gate item 7): stop a merge that needs a decision and go back; Undo brings
// the conflict back.

import { expect, test } from "@playwright/test";

import { conflictedMerge, confirmWizard, palette, reopenPanel } from "./helpers";

test("stops a conflicted merge and goes back to where it started", async ({ page }) => {
  await conflictedMerge(page);
  await reopenPanel(page);
  await expect(page.locator('[data-testid="git-file"][data-path$="health.yml"]')).toBeVisible();

  await palette(page, "Git: Stop the merge");
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("Stops the");
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText("Stop and go back");
  await confirmWizard(page, { again: true, done: false });
  await expect(wizard.getByTestId("git-wizard-result")).toContainText("went back");
  await page.getByTestId("git-wizard-undo").click();
  await expect(wizard.getByTestId("git-wizard-result")).toContainText("Undone");
  await page.getByTestId("git-wizard-done").click();
  await expect(page.locator('[data-testid="git-file"][data-path$="health.yml"]')).toBeVisible();
});

test("has nothing to stop without a merge", async ({ page }) => {
  const { openGitPanel } = await import("./helpers");
  await openGitPanel(page, { scenario: "clean" });
  await palette(page, "Git: Stop the merge");
  await expect(page.getByTestId("git-wizard")).toContainText("Nothing is in progress");
  await expect(page.getByTestId("git-wizard-confirm")).toBeDisabled();
});
