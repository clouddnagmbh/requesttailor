// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: opening the working copy views on the mock backend.

import { expect, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
export type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

/** Opens the demo collection with the "clean" Git scenario and the Git panel. */
export async function openPanel(page: Page): Promise<void> {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario("clean");
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
}

/** The Changes view, opened from the panel's link. */
export async function openChanges(page: Page): Promise<void> {
  await openPanel(page);
  await page.getByTestId("git-all-changes").click();
  await expect(page.getByTestId("changes-view")).toBeVisible();
  await expect(page.getByTestId("changes-row").first()).toBeVisible();
}

export const row = (page: Page, path: string) => page.locator(`[data-testid="changes-row"][data-path="${path}"]`);
