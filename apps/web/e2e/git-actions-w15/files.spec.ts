// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D (gate item 7): restore chosen files from a commit and park only some files.
// The entry points are the working-copy and history views (strands B and C); here the actions
// run through the backend the way those views call them, and the panel shows the outcome.

import { expect, test } from "@playwright/test";

import type { W } from "../git-wizards/helpers";
import { commit, editRequest, openGitPanel, reopenPanel } from "./helpers";

test("restores a file from an earlier commit, then Undo brings back the edit", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await editRequest(page, "health.yml", "https://example.test/second");
  await commit(page, "second", ["health.yml"]);
  await editRequest(page, "health.yml", "https://example.test/my-edit");
  const plan = await page.evaluate(() => (window as unknown as W).__rt.backend.git.plan({ action: "restore-files", rev: "HEAD~1", paths: ["/health.yml"] }));
  expect(plan.destructive).toBe(true);
  expect(plan.warnings?.[0]).toContain("uncommitted");
  const res = await page.evaluate(() => (window as unknown as W).__rt.backend.git.apply({ action: "restore-files", rev: "HEAD~1", paths: ["/health.yml"] }));
  await reopenPanel(page);
  await expect(page.locator('[data-testid="git-file"][data-path$="health.yml"]')).toBeVisible();
  const url = () => page.evaluate(async () => (await (window as unknown as W).__rt.backend.getRequest("health.yml")).url);
  expect(await url()).not.toBe("https://example.test/my-edit");
  await page.evaluate((id) => (window as unknown as W).__rt.backend.git.undo(id), res.undo!.id);
  expect(await url()).toBe("https://example.test/my-edit");
});

test("parks only the chosen files", async ({ page }) => {
  await openGitPanel(page, { scenario: "clean" });
  await editRequest(page, "health.yml", "https://example.test/a");
  await editRequest(page, "users/list-users.yml", "https://example.test/b");
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.apply({ action: "stash", paths: ["/health.yml"], message: "just health" }));
  await reopenPanel(page);
  await expect(page.getByTestId("git-file")).toHaveCount(1);
  await expect(page.locator('[data-testid="git-file"][data-path$="list-users.yml"]')).toBeVisible();
  const stashes = await page.evaluate(() => (window as unknown as W).__rt.backend.git.stashes());
  expect(stashes.map((s) => [s.message, s.files])).toEqual([["just health", ["/health.yml"]]]);
});
