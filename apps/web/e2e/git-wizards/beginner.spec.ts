// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G, gate item 10 (with strand F): the beginner flow without Git words. Branch,
// change, commit, merge back, publish, all from the panel and its wizards, every step said in
// plain words, every step undoable where Git allows it.

import { expect, test, type Page } from "@playwright/test";

import { branchMenu, confirmWizard, gitCalls, newBranch, openGitPanel, syncMenu, type W } from "./helpers";

async function changeAndSave(page: Page, name: string, method: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.getByTestId("method-select").selectOption(method);
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();
}

test("branch, change, commit, merge back, publish: in plain words", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");

  // A new branch: the wizard explains, checks the name while typing and shows the plan.
  await branchMenu(page, "New branch…");
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard).toContainText("A line of work");
  await page.getByTestId("git-wizard-name").fill("feature..x");
  await expect(wizard).toContainText("cannot be used as a name");
  await expect(page.getByTestId("git-wizard-confirm")).toBeDisabled();
  await page.getByTestId("git-wizard-name").fill("Faster health check");
  await expect(page.getByTestId("git-wizard-name")).toHaveValue("faster-health-check");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("faster-health-check");
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText("Create and switch");
  await confirmWizard(page);
  await expect(page.getByTestId("git-branch")).toHaveText("faster-health-check");
  await expect(page.getByTestId("git-sync")).toContainText("Not published");
  await expect(page.getByTestId("git-sync-action")).toHaveText("Publish branch");

  // Change and commit.
  await changeAndSave(page, "Health", "POST");
  await page.getByTestId("git-message").fill("Health is a POST now");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();

  // Back to main and merge the branch in: the plan shows what comes over, by field.
  await branchMenu(page, "main");
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  await branchMenu(page, "Merge into main…");
  await page.getByTestId("git-wizard-from").selectOption("faster-health-check");
  await expect(wizard.getByTestId("git-wizard-merge")).toContainText("health.yml");
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText("Move forward");
  await confirmWizard(page, { done: false });
  await expect(page.getByTestId("git-wizard-undo")).toBeVisible();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-sync")).toHaveAttribute("data-ahead", "1");

  // Send it: the header's one action says what it does.
  await expect(page.getByTestId("git-sync-action")).toHaveText("Push 1");
  await page.getByTestId("git-sync-action").click();
  await expect(page.getByTestId("git-result")).toContainText("Pushed main");
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");
  expect(await gitCalls(page)).toEqual(expect.arrayContaining(["push"]));
});

test("publishing a new branch, then undoing the last Git action from the menu", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await newBranch(page, "feature/docs");
  await page.getByTestId("git-sync-action").click();
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");

  // The last undoable action was creating the branch? No: switching back is not journaled;
  // undo the creation of a second branch instead.
  await newBranch(page, "scratch", { switchTo: false });
  await syncMenu(page, /^Undo: /);
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("scratch");
  await page.getByTestId("git-wizard-confirm").click();
  await expect(wizard.getByTestId("git-wizard-result")).toBeVisible();
  await page.getByTestId("git-wizard-done").click();
  const branches = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.git.branches()).map((b) => b.name));
  expect(branches).not.toContain("scratch");
});

test("the wizard is keyboard only: type, Enter, Enter", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await page.getByTestId("git-branch").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByRole("menu")).toBeVisible();
  await page.keyboard.press("End");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-wizard-name")).toBeFocused();
  await page.keyboard.type("kbd");
  await expect(page.getByTestId("git-wizard-confirm")).toBeEnabled();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-wizard-done")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-wizard")).toBeHidden();
  await expect(page.getByTestId("git-branch")).toHaveText("kbd");
});
