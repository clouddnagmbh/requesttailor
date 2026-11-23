// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G, gate item 13: every situation in plain words, with an empty state or a
// guided choice, and "What does this mean?" for the Git word behind it (mock backend).

import { expect, test } from "@playwright/test";

import { confirmWizard, openGitPanel, syncMenu } from "./helpers";

/** Git words a beginner should not meet in the panel's own sentences (the glossary names them). */
const JARGON = /\b(HEAD|upstream|fast-forward|rebase|stash|detached|refs?\/|origin\/HEAD|cherry-pick|reset --)\b/;

async function panelText(page: import("@playwright/test").Page): Promise<string> {
  return page.getByTestId("git-panel").evaluate((el) => {
    const copy = el.cloneNode(true) as HTMLElement;
    // Names the user chose (branches, remotes, upstream labels) are not jargon.
    copy.querySelectorAll('[data-testid="git-gloss"], .upstream, [data-testid="git-branch"], [data-testid="git-ref-pill"]').forEach((n) => n.remove());
    return copy.innerText;
  });
}

test("no repository: an empty state with the two ways in", async ({ page }) => {
  await openGitPanel(page, { scenario: "none" });
  const empty = page.getByTestId("git-norepo");
  await expect(empty).toContainText("No version history yet");
  await expect(page.getByTestId("git-init")).toHaveText("Start a repository here");
  await expect(page.getByTestId("git-clone-open")).toBeVisible();
  expect(await panelText(page)).not.toMatch(JARGON);
});

test("an empty repository says what to do first", async ({ page }) => {
  await openGitPanel(page, { scenario: "empty" });
  await expect(page.getByTestId("git-branch")).toContainText("no commits yet");
  await expect(page.getByTestId("git-changes")).toContainText("Commit these files to start the history.");
});

test("no remote: the main action links a server, with the plain explanation", async ({ page }) => {
  await openGitPanel(page, { scenario: "no-remote" });
  await expect(page.getByTestId("git-sync")).toContainText("No remote");
  await expect(page.getByTestId("git-sync-action")).toHaveAttribute("data-action", "add-remote");
  await page.getByTestId("git-sync-action").click();
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard).toHaveAttribute("data-kind", "add-remote");
  await expect(wizard).toContainText("A copy of the repository on a server");
  await page.getByTestId("git-wizard-url").fill("https://git.example.com/team/api.git");
  await page.getByTestId("git-wizard-name").fill("origin");
  await confirmWizard(page);
  await expect(page.getByTestId("git-sync-action")).toHaveAttribute("data-action", "publish");
});

test("unpublished branch: one click publishes it", async ({ page }) => {
  await openGitPanel(page, { scenario: "unpublished" });
  await expect(page.getByTestId("git-sync")).toContainText("Not published");
  await expect(page.getByTestId("git-sync-action")).toHaveText("Publish branch");
  await page.getByTestId("git-sync-action").click();
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");
});

test("behind: the main action pulls and says how many", async ({ page }) => {
  await openGitPanel(page, { scenario: "behind" });
  await expect(page.getByTestId("git-sync")).toHaveAttribute("data-behind", "2");
  await expect(page.getByTestId("git-sync-action")).toHaveText("Pull 2");
  await page.getByTestId("git-sync-action").click();
  await expect(page.getByTestId("git-result")).toContainText("Pulled 2 commits");
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");
});

test("diverged: a banner with the two guided choices and the explanation", async ({ page }) => {
  await openGitPanel(page, { scenario: "diverged" });
  const banner = page.getByTestId("git-diverged");
  await expect(banner).toContainText("went apart");
  await expect(banner).toContainText("You have 1 new commit, the server has 1.");
  await expect(page.getByTestId("git-sync-action")).toHaveCount(0);
  await page.getByTestId("git-explain-diverged").click();
  await expect(banner.getByTestId("git-gloss")).toContainText("You and the server both have new commits");
  await expect(banner.getByTestId("git-gloss")).toContainText("In Git: diverged");
  expect(await panelText(page)).not.toMatch(JARGON);

  await page.getByTestId("git-diverged-merge").click();
  await expect(page.getByTestId("git-result")).toContainText("Merged");
  await expect(page.getByTestId("git-sync")).toHaveAttribute("data-ahead", "2");
});

test("diverged, take theirs: a backup, a second question and Undo", async ({ page }) => {
  await openGitPanel(page, { scenario: "diverged" });
  await page.getByTestId("git-diverged-theirs").click();
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard).toHaveAttribute("data-kind", "diverged");
  await expect(wizard.getByTestId("git-wizard-choice-theirs")).toHaveAttribute("data-selected", "true");
  await expect(wizard.getByTestId("git-wizard-backup")).toBeVisible();
  await expect(page.getByTestId("git-wizard-confirm")).toHaveText("Take theirs");
  await page.getByTestId("git-wizard-confirm").click();
  await expect(wizard.getByTestId("git-wizard-again")).toContainText("Undo can bring them back");
  await page.getByTestId("git-wizard-confirm-again").click();
  await expect(wizard.getByTestId("git-wizard-undo")).toBeVisible();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-sync")).toContainText("Up to date");

  await syncMenu(page, /^Undo: /);
  await page.getByTestId("git-wizard-confirm").click();
  await page.getByTestId("git-wizard-done").click();
  await expect(page.getByTestId("git-diverged")).toBeVisible();
});

test("detached: a banner, a branch here or back to a branch", async ({ page }) => {
  await openGitPanel(page, { scenario: "detached" });
  await expect(page.getByTestId("git-detached")).toContainText("You are looking at an older commit");
  await expect(page.getByTestId("git-sync")).toContainText("Not on a branch");
  await page.getByTestId("git-detached-branch").click();
  await page.getByTestId("git-wizard-name").fill("rescue");
  await confirmWizard(page);
  await expect(page.getByTestId("git-branch")).toHaveText("rescue");
  await expect(page.getByTestId("git-detached")).toHaveCount(0);
});

test("conflicts: a banner leads to the decision, the file badge says so", async ({ page }) => {
  await openGitPanel(page, { scenario: "conflict" });
  await syncMenu(page, /^Pull/);
  const banner = page.getByTestId("git-merging");
  await expect(banner).toContainText("The merge needs your decision");
  await expect(page.locator('[data-testid="git-file"][data-path="health.yml"]').getByTestId("git-file-state")).toHaveText("U");
  await expect(page.getByTestId("git-commit")).toBeDisabled();
  await page.getByTestId("git-resolve").click();
  await expect(page.getByTestId("git-view")).toBeVisible();
});

test("a blocked wizard says why and offers the way out", async ({ page }) => {
  await openGitPanel(page, { scenario: "showcase" });
  // Showcase has uncommitted changes; merging a branch that touches them is blocked.
  await page.getByTestId("git-branch").click();
  await page.getByRole("menuitem", { name: "Delete branch…" }).click();
  const wizard = page.getByTestId("git-wizard");
  await page.getByTestId("git-wizard-branch").selectOption("feature/auth");
  await expect(wizard.getByTestId("git-wizard-plan")).toContainText("Deletes the branch feature/auth");
  await page.getByTestId("git-wizard-cancel").click();
  await expect(wizard).toBeHidden();

  // A push while the server has new commits: blocked, the way out is the guided choice.
  await openGitPanel(page, { scenario: "behind" });
  await syncMenu(page, /^Push/);
  await expect(page.getByTestId("git-error")).toContainText("Pull first");
});

test("the help button explains the word behind the current state", async ({ page }) => {
  await openGitPanel(page, { scenario: "ahead" });
  await page.getByTestId("git-help").click();
  await expect(page.getByTestId("git-repo").getByTestId("git-gloss")).toContainText("To send");
  await page.getByTestId("git-help").click();
  await expect(page.getByTestId("git-gloss")).toHaveCount(0);
});
