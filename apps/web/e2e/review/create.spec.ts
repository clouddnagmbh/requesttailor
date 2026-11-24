// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand E: "Create pull request" against the mock backend: from the review list and
// from a branch in the graph; a draft, the listing afterwards, and the host's refusals shown
// as they are (a second pull request for the same branches). The providers are tested in
// crates/rt-app/tests/review_create.rs against a fake transport.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";

async function openReview(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario("showcase");
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("review");
  await dialog.getByRole("option").filter({ hasText: "Review: pull and merge requests" }).click();
  await page.getByTestId("review-host").fill("git.example.com");
  await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
  await page.getByTestId("review-add-host").click();
  await page.getByTestId("review-token").fill("mock-token");
  await page.getByTestId("review-save-token").click();
  await expect(page.getByTestId("review-row")).toHaveCount(2);
}

test("creates a draft pull request from the review list and finds it in the list", async ({ page }) => {
  await openReview(page);
  await page.getByTestId("review-new").click();
  await expect(page.getByTestId("new-pull")).toBeVisible();
  await expect(page.getByTestId("new-pull-repo")).toContainText("acme/demo-api");
  await page.getByTestId("new-pull-head").selectOption("feature/auth");
  await page.getByTestId("new-pull-base").selectOption("main");
  await expect(page.getByTestId("new-pull-title")).not.toHaveValue("");
  await page.getByTestId("new-pull-title").fill("Bearer auth for users");
  await page.getByTestId("new-pull-draft").check();
  await page.getByTestId("new-pull-create").click();
  await expect(page.getByTestId("new-pull-done")).toContainText("Bearer auth for users");
  await page.getByTestId("new-pull-close").click();
  await expect(page.getByTestId("review-row")).toHaveCount(3);
  await expect(page.getByTestId("review-row").filter({ hasText: "Bearer auth for users" }).getByTestId("review-draft")).toBeVisible();
});

test("says what the host refused: a second pull request for the same branches", async ({ page }) => {
  await openReview(page);
  for (const round of [1, 2]) {
    await page.getByTestId("review-new").click();
    await page.getByTestId("new-pull-head").selectOption("feature/auth");
    await page.getByTestId("new-pull-base").selectOption("main");
    await page.getByTestId("new-pull-title").fill(`Auth ${round}`);
    await page.getByTestId("new-pull-create").click();
    if (round === 1) await page.getByTestId("new-pull-close").click();
  }
  await expect(page.getByTestId("new-pull-error")).toContainText(/already/i);
  await expect(page.getByTestId("new-pull-title")).toHaveValue("Auth 2");
});

test("a local branch in the graph offers Create pull request", async ({ page }) => {
  await openReview(page);
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Git: History");
  await palette.getByRole("option").filter({ hasText: "Git: History" }).first().click();
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
  await page.getByTestId("graph-row").filter({ hasText: "Move user creation to v2" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "feature/auth", exact: true }).hover();
  await page.getByRole("menuitem", { name: /Create pull request/ }).click();
  await expect(page.getByTestId("new-pull")).toBeVisible();
  await expect(page.getByTestId("new-pull-head")).toHaveValue("feature/auth");
});
