// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand A2: the Git view against the mock backend's Git (`?backend=mock`,
// apps/ui/src/commands/mock-git.ts). The same view runs in both shells; the web shell's real
// Git is covered by e2e/shells/git*.e2e.ts.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

type Scenario = "none" | "clean" | "conflict";

async function openGit(page: Page, scenario: Scenario = "clean", identity = true) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(
    async ({ scenario, identity }) => {
      const git = (window as unknown as W).__rt.backend.git;
      git.scenario(scenario);
      if (identity) await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
    },
    { scenario, identity },
  );
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
}

/** Changes a request's method in the editor and saves (as a user would). */
async function changeAndSave(page: Page, name: string, method: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.getByTestId("method-select").selectOption(method);
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();
}

const files = (page: Page) => page.getByTestId("git-file");
const file = (page: Page, path: string) => files(page).filter({ hasText: path });
const gitCalls = (page: Page) => page.evaluate(() => [...(window as unknown as W).__rt.backend.git.calls]);

test("shows branch, sync state and changed files with their state", async ({ page }) => {
  await openGit(page);
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  await expect(page.getByTestId("git-sync")).toContainText("origin/main");
  await expect(page.getByTestId("git-clean")).toBeVisible();

  await changeAndSave(page, "Health", "POST");
  await expect(files(page)).toHaveCount(1);
  await expect(file(page, "health.yml").getByTestId("git-file-state")).toHaveText("M");
  await expect(file(page, "health.yml").getByTestId("git-file-check")).toBeChecked();
  await expect(page.getByTestId("git-log").getByRole("listitem")).toHaveText(["Initial collection"]);
});

test("clicking a file shows its diff with coloured added and removed lines", async ({ page }) => {
  await openGit(page);
  await changeAndSave(page, "Health", "POST");
  await file(page, "health.yml").getByTestId("git-file-open").click();
  const diff = page.getByTestId("git-diff");
  await expect(diff).toBeVisible();
  await expect(diff.getByTestId("git-diff-path")).toHaveText("health.yml");
  await expect(diff.getByTestId("git-hunk")).toHaveCount(1);
  await expect(diff.locator('[data-kind="removed"]')).toHaveText(["-  method: GET"]);
  await expect(diff.locator('[data-kind="added"]')).toHaveText(["+  method: POST"]);
  const color = (kind: string) =>
    diff.locator(`[data-kind="${kind}"]`).first().evaluate((e) => getComputedStyle(e).backgroundColor);
  expect(await color("added")).not.toBe(await color("removed"));
  expect(await color("added")).not.toBe(await diff.locator('[data-kind="context"]').first().evaluate((e) => getComputedStyle(e).backgroundColor));
  await page.keyboard.press("Escape");
  await expect(diff).toBeHidden();
});

test("commits only the checked files", async ({ page }) => {
  await openGit(page);
  await changeAndSave(page, "Health", "POST");
  await changeAndSave(page, "List users", "PUT");
  await expect(files(page)).toHaveCount(2);
  const commit = page.getByTestId("git-commit");
  await expect(commit).toBeDisabled();
  await page.getByTestId("git-message").fill("Health is a POST now");
  await expect(commit).toBeEnabled();
  await file(page, "users/list-users.yml").getByTestId("git-file-check").uncheck();
  await commit.click();
  await expect(files(page)).toHaveCount(1);
  await expect(files(page)).toContainText("users/list-users.yml");
  await expect(page.getByTestId("git-message")).toHaveValue("");
  await expect(page.getByTestId("git-sync")).toContainText("↑1");
  await expect(page.getByTestId("git-log").getByRole("listitem").first()).toHaveText("Health is a POST now");
  expect(await gitCalls(page)).toContain("commit health.yml");

  // Unchecking everything disables Commit again.
  await page.getByTestId("git-message").fill("more");
  await file(page, "users/list-users.yml").getByTestId("git-file-check").uncheck();
  await expect(commit).toBeDisabled();
});

test("asks for the author identity before the first commit", async ({ page }) => {
  await openGit(page, "clean", false);
  await changeAndSave(page, "Health", "POST");
  await page.getByTestId("git-message").fill("first own commit");
  await page.getByTestId("git-commit").click();
  const form = page.getByTestId("git-identity");
  await expect(form).toBeVisible();
  await form.getByTestId("git-identity-name").fill("Grace Hopper");
  await form.getByTestId("git-identity-email").fill("not-an-email");
  await form.getByTestId("git-identity-save").click();
  await expect(page.getByTestId("git-error")).toContainText("valid email");
  await form.getByTestId("git-identity-email").fill("grace@example.com");
  await form.getByTestId("git-identity-save").click();
  await expect(form).toBeHidden();
  await expect(page.getByTestId("git-log").getByRole("listitem").first()).toHaveText("first own commit");
  expect((await page.evaluate(() => (window as unknown as W).__rt.backend.git.log(1)))[0].author).toBe("Grace Hopper");
});

test("creates a branch, switches back and forth, and the tree follows", async ({ page }) => {
  await openGit(page);
  await page.getByTestId("git-new-branch").click();
  await page.getByTestId("git-new-branch-name").fill("feature/extra");
  await page.getByTestId("git-new-branch-name").press("Enter");
  await expect(page.getByTestId("git-branch")).toHaveText("feature/extra");

  // A new request on the branch, saved and committed.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("new-request").click();
  await page.getByTestId("new-request-name").fill("Only on feature");
  await page.getByTestId("new-request-name").press("Enter");
  await page.getByTestId("save").click();
  await page.getByTestId("view-git").click();
  await expect(file(page, "only-on-feature.yml").getByTestId("git-file-state")).toHaveText("U");
  await page.getByTestId("git-message").fill("Add feature request");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();

  await page.getByTestId("git-branch-select").selectOption("main");
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  await page.getByTestId("view-requests").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Only on feature" })).toHaveCount(0);
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-branch-select").selectOption("feature/extra");
  await page.getByTestId("view-requests").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Only on feature" })).toHaveCount(1);
});

test("unsaved changes block switching branches and pulling, and say why", async ({ page }) => {
  await openGit(page);
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("method-select").selectOption("DELETE");
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-blocked")).toContainText("Save");
  await expect(page.getByTestId("git-branch-select")).toBeDisabled();
  await expect(page.getByTestId("git-pull")).toBeDisabled();
  await page.getByTestId("save").click();
  await expect(page.getByTestId("git-blocked")).toBeHidden();
  await expect(page.getByTestId("git-pull")).toBeEnabled();
  // After saving, the status shows the change without reopening the view.
  await expect(file(page, "health.yml")).toBeVisible();
});

test("push and pull show progress and their result", async ({ page }) => {
  await openGit(page);
  await changeAndSave(page, "Health", "POST");
  await page.getByTestId("git-message").fill("local");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-sync")).toContainText("↑1");
  await page.getByTestId("git-push").click();
  await expect(page.getByTestId("git-result")).toHaveText("Pushed main to origin/main.");
  await expect(page.getByTestId("git-sync")).not.toContainText("↑1");

  // A colleague pushed: pull fast-forwards and the tree reloads.
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    b.git.remoteCommit({ "health.yml": { ...spec, name: "Health (remote)" } });
  });
  await page.getByTestId("git-pull").click();
  await expect(page.getByTestId("git-result")).toHaveText("Pulled 1 commit (fast-forward).");
  await page.getByTestId("view-requests").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Health (remote)" })).toBeVisible();
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-pull").click();
  await expect(page.getByTestId("git-result")).toHaveText("Already up to date.");

  // Rejected push: the remote moved on.
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    b.git.remoteCommit({ "other.yml": await b.getRequest("health.yml") });
  });
  await changeAndSave(page, "Health", "PATCH");
  await page.getByTestId("git-message").fill("again");
  await page.getByTestId("git-commit").click();
  await page.getByTestId("git-push").click();
  await expect(page.getByTestId("git-error")).toContainText("Pull first");
  await page.getByTestId("git-pull").click();
  await expect(page.getByTestId("git-result")).toHaveText("Merged origin/main.");
});

test("a pull with conflicts: ours and theirs side by side, resolve, commit the merge", async ({ page }) => {
  await openGit(page, "conflict");
  await expect(page.getByTestId("git-sync")).toContainText("↑1");
  await page.getByTestId("git-pull").click();
  await expect(page.getByTestId("git-result")).toContainText("Conflicts in 1 file");
  await expect(page.getByTestId("git-merging")).toBeVisible();
  const row = file(page, "health.yml");
  await expect(row.getByTestId("git-file-state")).toHaveText("C");
  await expect(page.getByTestId("git-commit")).toBeDisabled();

  await row.getByTestId("git-file-open").click();
  const view = page.getByTestId("git-conflict");
  await expect(view).toBeVisible();
  await expect(view.getByTestId("git-conflict-ours")).toContainText("/health/local");
  await expect(view.getByTestId("git-conflict-theirs")).toContainText("/health/remote");
  await view.getByTestId("git-use-theirs").click();
  await expect(view).toBeHidden();

  await expect(row.getByTestId("git-file-state")).toHaveText("M");
  await expect(page.getByTestId("git-message")).toHaveValue("Merge origin/main into main");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-merging")).toBeHidden();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  // The tree shows the resolved request.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  expect((await page.evaluate(() => (window as unknown as W).__rt.backend.getRequest("health.yml"))).url).toBe(
    "{{baseUrl}}/health/remote",
  );
});

test("missing credentials: prompt, store, retry", async ({ page }) => {
  await openGit(page);
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.requireAuth());
  await changeAndSave(page, "Health", "POST");
  await page.getByTestId("git-message").fill("needs auth");
  await page.getByTestId("git-commit").click();
  await page.getByTestId("git-push").click();
  const prompt = page.getByTestId("git-credentials");
  await expect(prompt).toBeVisible();
  await expect(prompt).toContainText("git.example.com");
  await prompt.getByTestId("git-cred-username").fill("ada");
  await prompt.getByTestId("git-cred-password").fill("token-123");
  await prompt.getByTestId("git-cred-save").click();
  await expect(prompt).toBeHidden();
  await expect(page.getByTestId("git-result")).toHaveText("Pushed main to origin/main.");
  expect(await gitCalls(page)).toEqual(expect.arrayContaining(["setCredentials git.example.com"]));
  expect((await gitCalls(page)).filter((c) => c === "push")).toHaveLength(2);
});

test("a folder without a repository offers to initialize one", async ({ page }) => {
  await openGit(page, "none");
  await expect(page.getByTestId("git-norepo")).toBeVisible();
  await page.getByTestId("git-init").click();
  await expect(page.getByTestId("git-norepo")).toBeHidden();
  await expect(page.getByTestId("git-branch")).toHaveText("main (no commits yet)");
  await expect(files(page)).not.toHaveCount(0);
  await expect(file(page, "health.yml").getByTestId("git-file-state")).toHaveText("U");
  await page.getByTestId("git-message").fill("Initial commit");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-clean")).toBeVisible();
  await expect(page.getByTestId("git-branch")).toHaveText("main");
});

test("clone: URL, then the cloned collection opens", async ({ page }) => {
  await openGit(page, "none");
  await page.getByTestId("git-clone-url").fill("https://git.example.com/acme/demo-api.git");
  await page.getByTestId("git-clone").click();
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  expect(await gitCalls(page)).toContain("clone https://git.example.com/acme/demo-api.git");
});

test("the Git view is keyboard accessible", async ({ page }) => {
  await openGit(page);
  await changeAndSave(page, "Health", "POST");
  await file(page, "health.yml").getByTestId("git-file-open").focus();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-diff")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("git-diff")).toBeHidden();
  await expect(file(page, "health.yml").getByTestId("git-file-open")).toBeFocused();
  await page.getByTestId("git-message").focus();
  await page.keyboard.type("via keyboard");
  await page.keyboard.press("Tab");
  await expect(page.getByTestId("git-commit")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("git-clean")).toBeVisible();
});
