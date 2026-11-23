// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G, gate item 11: every destructive action keeps a backup, asks a second time
// with the danger button, and its result offers Undo that brings everything back (mock backend).

import { expect, test, type Page } from "@playwright/test";

import { openGitPanel, syncMenu, type W } from "./helpers";

const backups = (page: Page) =>
  page.evaluate(() => [...((window as unknown as W).__rt.backend.git as unknown as { wave14Actions: { backupRefs(): Map<string, string> } }).wave14Actions.backupRefs().keys()]);
const branches = (page: Page) =>
  page.evaluate(async () => (await (window as unknown as W).__rt.backend.git.branches()).filter((b) => !b.remote).map((b) => b.name));
const changed = (page: Page) => page.evaluate(async () => (await (window as unknown as W).__rt.backend.git.status()).files.map((f) => f.path).sort());

/** The plan shows the backup; the first click asks again, the second runs; Undo is offered. */
async function confirmDestructive(page: Page): Promise<void> {
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard.getByTestId("git-wizard-backup")).toBeVisible();
  await expect(page.getByTestId("git-wizard-confirm")).toHaveAttribute("data-destructive", "true");
  await page.getByTestId("git-wizard-confirm").click();
  await expect(wizard.getByTestId("git-wizard-again")).toBeVisible();
  await expect(page.getByTestId("git-wizard-confirm-again")).toBeFocused();
  await page.getByTestId("git-wizard-confirm-again").click();
  await expect(wizard.getByTestId("git-wizard-undo")).toBeVisible();
}

async function undoFromResult(page: Page): Promise<void> {
  await page.getByTestId("git-wizard-undo").click();
  await expect(page.getByTestId("git-wizard-result")).toBeVisible();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-wizard")).toBeHidden();
}

test("discard changes: backup, second question, Undo brings the files back", async ({ page }) => {
  await openGitPanel(page, { scenario: "showcase" });
  const before = await changed(page);
  expect(before.length).toBeGreaterThan(0);
  await page.getByTestId("git-changes-menu").click();
  await page.getByRole("menuitem", { name: "Discard changes…" }).click();
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText(`Discard ${before.length} changes`);
  await confirmDestructive(page);
  expect(await changed(page)).toEqual([]);
  expect((await backups(page)).length).toBeGreaterThan(0);
  await undoFromResult(page);
  expect(await changed(page)).toEqual(before);
});

test("moving a branch back and throwing work away: Undo restores the commit", async ({ page }) => {
  await openGitPanel(page, { scenario: "ahead" });
  await expect(page.getByTestId("git-sync")).toHaveAttribute("data-ahead", "1");
  await page.getByTestId("git-commit-menu").first().click();
  await page.getByRole("menuitem", { name: "Branch from here…" }).waitFor();
  await page.keyboard.press("Escape");
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("Git: Move branch back");
  await page.keyboard.press("Enter");
  await page.getByTestId("git-wizard-mode-discard").click();
  await confirmDestructive(page);
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");
  await syncMenu(page, /^Undo: /);
  await page.getByTestId("git-wizard-confirm").click();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-sync")).toHaveAttribute("data-ahead", "1");
});

test("deleting a branch with commits found nowhere else: blocked, delete anyway, Undo", async ({ page }) => {
  await openGitPanel(page, { scenario: "unpublished" });
  // A commit only on feature/login.
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "{{baseUrl}}/login-only" });
    await b.save();
    await b.git.commit("Only on login", ["health.yml"]);
    await b.git.checkout("main");
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-branch").click();
  await page.getByRole("menuitem", { name: "Delete branch…" }).click();
  await page.getByTestId("git-wizard-branch").selectOption("feature/login");
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard.getByTestId("git-wizard-blocked")).toHaveAttribute("data-reason", "not-merged");
  await expect(wizard.getByTestId("git-wizard-warning")).toContainText("only on feature/login");
  await page.getByTestId("git-wizard-way-out").click();
  await confirmDestructive(page);
  expect(await branches(page)).not.toContain("feature/login");
  await undoFromResult(page);
  expect(await branches(page)).toContain("feature/login");
});

test("parked changes: park, bring back, delete them, and Undo the delete", async ({ page }) => {
  await openGitPanel(page, { scenario: "showcase" });
  const before = await changed(page);
  await page.getByTestId("git-changes-menu").click();
  await page.getByRole("menuitem", { name: "Park changes…" }).click();
  await page.getByTestId("git-wizard-message").fill("Half-done users");
  await expect(page.getByTestId("git-wizard-plan")).toBeVisible();
  await page.getByTestId("git-wizard-confirm").click();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  const list = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.git.stashes()).map((s) => s.message));
  expect(list).toEqual(["Half-done users"]);

  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("Git: Delete parked changes");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-wizard-stash")).toContainText("Half-done users");
  await confirmDestructive(page);
  await undoFromResult(page);

  await syncMenu(page, /^Bring back parked changes/);
  await expect(page.getByTestId("git-wizard-plan")).toBeVisible();
  await page.getByTestId("git-wizard-confirm").click();
  await page.getByTestId("git-wizard-done").click();
  expect(await changed(page)).toEqual(before);
});

test("undoing a commit and copying one here: new commits, Undo moves the branch back", async ({ page }) => {
  await openGitPanel(page, { scenario: "ahead" });
  const head = await page.evaluate(() => (window as unknown as W).__rt.backend.git.revParse("HEAD"));
  await page.getByTestId("git-commit-menu").first().click();
  await page.getByRole("menuitem", { name: "Undo this commit…" }).click();
  await expect(page.getByTestId("git-wizard-plan")).toContainText("new commit");
  await page.getByTestId("git-wizard-confirm").click();
  await expect(page.getByTestId("git-wizard-undo")).toBeVisible();
  await undoFromResult(page);
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.git.revParse("HEAD"))).toBe(head);
});
