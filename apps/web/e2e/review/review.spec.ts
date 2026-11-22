// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand E: pull-request review against the mock backend (`?backend=mock`,
// apps/ui/src/commands/mock-review.ts): add a self-hosted host, give a token (a wrong one is
// refused and never shown), list, open a PR, read its semantic diff, run the changed
// requests, comment on a field, request changes, approve. The same view runs against the
// real core and a fake Git host in review.e2e.ts.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const TOKEN = "mock-token";
const WRONG = "wrong-token-VALUE-42";

async function setup(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("review");
  await dialog.getByRole("option").filter({ hasText: "Review: pull and merge requests" }).click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "review");
}

async function signIn(page: Page, token: string) {
  await page.getByTestId("review-token").fill(token);
  await page.getByTestId("review-save-token").click();
}

test("adds a host, refuses a wrong token, lists the pull requests", async ({ page }) => {
  await setup(page);
  // The mock's origin is on a host the app does not know.
  await expect(page.getByTestId("review-no-host")).toBeVisible();
  await page.getByTestId("review-host").fill("git.example.com");
  await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
  await page.getByTestId("review-add-host").click();
  await expect(page.getByTestId("review-hosts")).toContainText("git.example.com");

  // No token yet: the form asks for one. A wrong one is refused with the host's reason, and
  // the value is nowhere on the page.
  await expect(page.getByTestId("review-token")).toBeVisible();
  await signIn(page, WRONG);
  await expect(page.getByTestId("review-token-message")).toContainText("401");
  expect(await page.content()).not.toContain(WRONG);
  await expect(page.getByTestId("review-token")).toHaveValue("");

  await signIn(page, TOKEN);
  const rows = page.getByTestId("review-row");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("#1");
  await expect(rows.first()).toContainText("feature/currency → main");
  await expect(rows.nth(1).getByTestId("review-draft")).toBeVisible();
  await expect(page.getByTestId("review-repo")).toHaveText("git.example.com · acme/demo-api");
  expect(await page.content()).not.toContain(TOKEN);
});

test("opens a pull request: diff, run, comment on a field, review", async ({ page }) => {
  await setup(page);
  await page.getByTestId("review-host").fill("git.example.com");
  await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
  await page.getByTestId("review-add-host").click();
  await signIn(page, TOKEN);
  await page.getByTestId("review-row").first().click();

  await expect(page.getByTestId("review-title")).toHaveText("#1 Add a deeper health check and a ping");
  await expect(page.getByTestId("review-state")).toHaveText("Open");
  // CI: one failed check makes it a failure.
  await expect(page.getByTestId("review-ci")).toHaveAttribute("data-state", "failure");
  await expect(page.getByTestId("review-check")).toHaveCount(3);

  // The semantic diff: merge base → head.
  const changes = page.getByTestId("sd-change");
  await expect(changes).toHaveCount(3);
  await expect(changes.filter({ hasText: "health.yml" })).toHaveAttribute("data-kind", "modified");
  await expect(changes.filter({ hasText: "ping.yml" })).toHaveAttribute("data-kind", "added");

  // Run the changed requests at the head.
  await page.getByTestId("review-run").click();
  const steps = page.getByTestId("review-step");
  await expect(steps).toHaveCount(3);
  await expect(page.getByTestId("review-run-summary")).toContainText("of 3 passed");
  await expect(steps.filter({ hasText: "ping.yml" })).toBeVisible();

  // A reviewer's earlier comment sits under its file, mapped to the field.
  const group = page.getByTestId("review-comment-group").filter({ hasText: "Is this URL right for staging?" });
  await expect(group.getByTestId("review-comment-field")).toBeVisible();
  await expect(page.getByTestId("review-comment-group").first()).toHaveAttribute("data-file", "");

  // Comment on a changed field of health.yml.
  const health = changes.filter({ hasText: "health.yml" });
  await health.getByTestId("sd-comment").first().click();
  await expect(page.getByTestId("review-composer")).toContainText("health.yml:");
  await page.getByTestId("review-comment-text").fill("Does staging have /health?deep=true?");
  await page.getByTestId("review-comment-send").click();
  await expect(page.getByTestId("review-composer")).toBeHidden();
  const mine = page.getByTestId("review-comment").filter({ hasText: "Does staging have" });
  await expect(mine).toBeVisible();
  await expect(mine.getByTestId("review-comment-field")).toBeVisible();

  // Request changes needs a reason; approving does not.
  await page.getByTestId("review-request-changes").click();
  await expect(page.getByTestId("review-action-error")).toContainText("422");
  await page.getByTestId("review-body").fill("Please check staging first.");
  await page.getByTestId("review-request-changes").click();
  await expect(page.getByTestId("review-sent")).toHaveText("Changes requested.");
  await page.getByTestId("review-approve").click();
  await expect(page.getByTestId("review-sent")).toHaveText("Approved.");

  // Layout: nothing overflows horizontally at a normal window size.
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  expect(overflow).toBe(false);
  await page.screenshot({ path: process.env.RT_REVIEW_SHOT ?? "test-results/review-pull.png", fullPage: true });

  await page.getByTestId("review-back").click();
  await expect(page.getByTestId("review-list")).toBeVisible();
});

test("a removed token asks for one again", async ({ page }) => {
  await setup(page);
  await page.getByTestId("review-host").fill("git.example.com");
  await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
  await page.getByTestId("review-add-host").click();
  await signIn(page, TOKEN);
  await expect(page.getByTestId("review-row")).toHaveCount(2);
  await page.getByTestId("review-settings").click();
  await expect(page.getByTestId("review-token-set")).toBeVisible();
  await page.getByTestId("review-remove-token").click();
  await expect(page.getByTestId("review-token")).toBeVisible();
});
