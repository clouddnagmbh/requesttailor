// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand B (gate item 2): a pull that stops on a request both sides changed is
// resolved per field in the app, then committed. Against the mock backend's Git
// (`?backend=mock`): its pull conflicts per request, its merge (mock-merges.ts) per field, as
// the core's `rt_format::merge` does on desktop and web (tested in Rust and in
// apps/web/src/git/merge.test.ts).

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend, RequestSpec } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = process.platform === "darwin" ? "Meta" : "Control";

/** Colleague and we both change health.yml: `theirs` on the remote, `ours` committed here. */
async function diverged(page: Page, theirs: Partial<RequestSpec>, ours: Partial<RequestSpec>) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(
    async ({ theirs, ours }) => {
      const b = (window as unknown as W).__rt.backend;
      b.git.scenario("clean");
      await b.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
      const base = await b.getRequest("health.yml");
      b.git.remoteCommit({ "health.yml": { ...base, ...theirs } }, "Colleague: health");
      await b.updateRequest("health.yml", { ...base, ...ours });
      await b.save();
      await b.git.commit("Mine: health", ["health.yml"]);
    },
    { theirs, ours },
  );
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-sync-menu").click();
  await page.getByRole("menuitem", { name: /^Pull/ }).click();
  await expect(page.getByTestId("git-merging")).toContainText("The merge needs your decision");
}

const health = (page: Page) => page.evaluate(() => (window as unknown as W).__rt.backend.getRequest("health.yml"));

test("the merge view decides each conflicted field; the rest is merged; commit concludes", async ({ page }) => {
  await diverged(
    page,
    { method: "POST", url: "{{baseUrl}}/health/remote", headers: [{ name: "X-Trace", value: "theirs", enabled: true }] },
    { url: "{{baseUrl}}/health/local", headers: [{ name: "X-Trace", value: "ours", enabled: true }] },
  );

  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Resolve merge conflicts");
  await page.keyboard.press("Enter");
  const view = page.getByTestId("merge-view");
  await expect(view).toBeVisible();
  // One row per field both sides changed; the method only theirs changed is not a conflict.
  const fields = view.getByTestId("merge-field");
  await expect(fields).toHaveText(["url", "headers › X-Trace"]);
  await expect(view.getByTestId("merge-resolve")).toBeDisabled();

  const file = view.locator('[data-testid="merge-file"][data-path="health.yml"]');
  await expect(file).toContainText("{{baseUrl}}/health/local");
  await expect(file).toContainText("{{baseUrl}}/health/remote");
  const radios = (side: "ours" | "theirs") => view.getByTestId(`merge-choose-${side}`);
  await radios("theirs").nth(0).check(); // url: theirs
  await expect(view.getByTestId("merge-resolve")).toBeDisabled();
  await radios("ours").nth(1).check(); // header: ours
  await view.getByTestId("merge-resolve").click();
  await expect(view.getByTestId("merge-done")).toContainText("All conflicts are resolved");

  const spec = await health(page);
  expect([spec.method, spec.url, spec.headers.find((h) => h.name === "X-Trace")?.value]).toEqual([
    "POST",
    "{{baseUrl}}/health/remote",
    "ours",
  ]);

  await page.getByTestId("git-view-close").click();
  await page.getByTestId("view-git").click();
  await expect(page.locator(`[data-testid="git-file"][data-path="health.yml"]`).getByTestId("git-file-state")).toHaveText("M");
  await expect(page.getByTestId("git-message")).toHaveValue("Merge origin/main into main");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-merging")).toBeHidden();
  await expect(page.getByTestId("git-clean")).toBeVisible();
});

test("the conflict dialog of a file offers the fields first, the whole versions below", async ({ page }) => {
  await diverged(page, { method: "PUT", url: "{{baseUrl}}/health/remote" }, { url: "{{baseUrl}}/health/local" });
  await page.locator(`[data-testid="git-file"][data-path="health.yml"]`).getByTestId("git-file-open").click();
  const dialog = page.getByTestId("git-conflict");
  await expect(dialog.getByTestId("merge-field")).toHaveText(["url"]);
  await expect(dialog.getByTestId("git-use-ours")).toBeVisible();
  await dialog.getByTestId("merge-choose-ours").check();
  await dialog.getByTestId("merge-resolve").click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(`[data-testid="git-file"][data-path="health.yml"]`).getByTestId("git-file-state")).toHaveText("M");
  const spec = await health(page);
  expect([spec.method, spec.url]).toEqual(["PUT", "{{baseUrl}}/health/local"]);
});

test("a request changed in different fields is merged by Resolve without a choice", async ({ page }) => {
  await diverged(page, { method: "DELETE" }, { url: "{{baseUrl}}/health/local" });
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Resolve merge conflicts");
  await page.keyboard.press("Enter");
  const view = page.getByTestId("merge-view");
  await expect(view.getByTestId("merge-clean")).toContainText("health.yml");
  await expect(view.getByTestId("merge-field")).toHaveCount(0);
  await view.getByTestId("merge-resolve").click();
  await expect(view.getByTestId("merge-done")).toBeVisible();
  const spec = await health(page);
  expect([spec.method, spec.url]).toEqual(["DELETE", "{{baseUrl}}/health/local"]);
});
