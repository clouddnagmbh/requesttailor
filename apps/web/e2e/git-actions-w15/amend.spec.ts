// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D (gate item 7): change the last commit from its wizard; the plan says what
// is replaced, a backup is kept, Undo brings the old commit back; a commit that is on the
// server is refused in plain words.

import { expect, test } from "@playwright/test";

import { commit, confirmWizard, editRequest, openGitPanel, palette } from "./helpers";

test("changes the last commit's message, with a backup and Undo", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await editRequest(page, "users/list-users.yml", "https://example.test/local");
  await commit(page, "local chnge", ["users/list-users.yml"]);

  await palette(page, "Git: Change last commit");
  const wizard = page.getByTestId("git-wizard");
  await page.getByTestId("git-wizard-message").fill("local change");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("Replaces the last commit");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("local change");
  await expect(wizard.getByTestId("git-wizard-backup")).toBeVisible();
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText("Change last commit");
  await confirmWizard(page, { done: false });
  await expect(wizard.getByTestId("git-wizard-result")).toContainText("local change");
  await page.getByTestId("git-wizard-undo").click();
  await expect(wizard.getByTestId("git-wizard-result")).toContainText("Undone");
  await page.getByTestId("git-wizard-done").click();
  const log = await page.evaluate(() => (window as unknown as import("../git-wizards/helpers").W).__rt.backend.git.log(1));
  expect(log[0].summary).toBe("local chnge");
});

test("refuses a commit that is on the server", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await palette(page, "Git: Change last commit");
  await page.getByTestId("git-wizard-message").fill("new words");
  await expect(page.getByTestId("git-wizard")).toContainText("already on the server");
  await expect(page.getByTestId("git-wizard-confirm")).toBeDisabled();
});
