// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand D: drive the mock's Git for the new actions (apps/ui/src/commands/mock-git*.ts).

import { expect, type Page } from "@playwright/test";

import { openGitPanel, type W } from "../git-wizards/helpers";

export { confirmWizard, openGitPanel } from "../git-wizards/helpers";

const MOD = "ControlOrMeta";

/** Runs a palette entry such as "Git: Change last commit…". */
export async function palette(page: Page, label: string): Promise<void> {
  await page.keyboard.press(`${MOD}+k`);
  const box = page.getByRole("dialog", { name: "Command palette" });
  await box.getByRole("combobox").fill(label);
  await box.getByRole("option").filter({ hasText: label }).first().click();
}

/** Edits a request's URL in the mock backend and saves it, as the editor would. */
export async function editRequest(page: Page, path: string, url: string): Promise<void> {
  await page.evaluate(
    async ({ path, url }) => {
      const b = (window as unknown as W).__rt.backend;
      const spec = await b.getRequest(path);
      await b.updateRequest(path, { ...spec, url });
      await b.save();
    },
    { path, url },
  );
}

export const commit = (page: Page, message: string, paths: string[]) =>
  page.evaluate(({ message, paths }) => (window as unknown as W).__rt.backend.git.commit(message, paths), { message, paths });

/** A conflicted merge: `other` and `main` both changed health.yml. */
export async function conflictedMerge(page: Page): Promise<void> {
  await openGitPanel(page, { scenario: "clean" });
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true }));
  await editRequest(page, "health.yml", "https://example.test/theirs");
  await commit(page, "theirs", ["health.yml"]);
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.apply({ action: "switch", target: "main" }));
  await editRequest(page, "health.yml", "https://example.test/ours");
  await commit(page, "ours", ["health.yml"]);
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.apply({ action: "merge", from: "other" }));
}

export async function reopenPanel(page: Page): Promise<void> {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
}
