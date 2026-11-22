// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand D: the history of a request against the mock backend (`?backend=mock`): its
// versions and what each commit changed, blame per field, the test results recorded under a
// commit, and sending and restoring an old version through the editor.

import { expect, test, type Page } from "@playwright/test";

type Rt = {
  __rt: {
    backend: {
      git: { setIdentity(i: object): Promise<void>; commit(message: string, paths: string[]): Promise<unknown> };
      timeline: { record(rev: string, r: object): Promise<void> };
    };
  };
};

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    await (window as unknown as Rt).__rt.backend.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
}

/** Health becomes a POST and is committed, so its file has two versions. */
async function commitHealthChange(page: Page) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("method-select").selectOption("POST");
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.evaluate(() => (window as unknown as Rt).__rt.backend.git.commit("Health is a POST", ["health.yml"]));
}

async function openHistory(page: Page) {
  await page.keyboard.press("ControlOrMeta+K");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("history of this");
  await palette.getByRole("option", { name: /History of this request/ }).click();
  await expect(page.getByTestId("request-timeline")).toHaveAttribute("data-path", "health.yml");
}

test("the history lists versions with their field changes and blame names the commit", async ({ page }) => {
  await openCollection(page);
  await commitHealthChange(page);
  await openHistory(page);

  await expect(page.getByTestId("tl-summary")).toHaveText("2 versions");
  const entries = page.getByTestId("tl-entry");
  await expect(entries).toHaveCount(2);
  await expect(entries.first().getByTestId("tl-commit")).toHaveText("Health is a POST");
  await expect(entries.first().getByTestId("tl-change")).toHaveText("modified");
  const changed = entries.first().getByTestId("tl-field");
  await expect(changed).toHaveCount(1);
  await expect(changed).toHaveAttribute("data-path", "method");
  await expect(changed).toContainText("GET");
  await expect(changed).toContainText("POST");
  await expect(entries.last().getByTestId("tl-change")).toHaveText("added");

  // Nothing overflows the view.
  const view = await page.getByTestId("request-timeline").boundingBox();
  const card = await entries.first().boundingBox();
  expect(view && card && card.x >= view.x && card.x + card.width <= view.x + view.width + 1).toBe(true);
  await page.screenshot({ path: test.info().outputPath("timeline.png") });

  await page.getByTestId("tl-mode-blame").click();
  const method = page.locator('[data-testid="tl-blame-row"][data-path="method"]');
  await expect(method.getByTestId("tl-blame-commit")).toContainText("Health is a POST");
  const url = page.locator('[data-testid="tl-blame-row"][data-path="url"]');
  await expect(url.getByTestId("tl-blame-commit")).not.toContainText("Health is a POST");
  await page.screenshot({ path: test.info().outputPath("blame.png") });
});

test("test results recorded under a commit show in its entry", async ({ page }) => {
  await openCollection(page);
  await commitHealthChange(page);
  await page.evaluate(async () => {
    const t = (window as unknown as Rt).__rt.backend.timeline;
    await t.record("HEAD", { request: "health.yml", environment: "Local", tests: [{ name: "status is 200", passed: false }] });
    await t.record("HEAD~1", { request: "health.yml", environment: "Local", tests: [{ name: "status is 200", passed: true }] });
  });
  await openHistory(page);
  const entries = page.getByTestId("tl-entry");
  await expect(entries.first().getByTestId("tl-result").first()).toHaveText("Failed");
  await expect(entries.last().getByTestId("tl-result").first()).toHaveText("Passed");
  await entries.first().getByTestId("tl-tests").locator("summary").click();
  await expect(entries.first().getByTestId("tl-run")).toContainText("fail: status is 200");
  await page.screenshot({ path: test.info().outputPath("results.png") });
});

test("an old version can be viewed, sent and restored through the editor", async ({ page }) => {
  await openCollection(page);
  await commitHealthChange(page);
  await openHistory(page);
  const old = page.getByTestId("tl-entry").last();

  await old.getByTestId("tl-view").click();
  await expect(old.getByTestId("tl-version")).toContainText("GET");
  await old.getByTestId("tl-version-close").click();

  // Send the old version: it lands in the editor, unsaved, and is sent.
  await old.getByTestId("tl-send").click();
  await expect(page.getByTestId("tl-notice")).toContainText("Sent version");
  await page.getByTestId("git-view-close").click();
  await expect(page.getByTestId("method-select")).toHaveValue("GET");
  await expect(page.getByTestId("save")).toBeEnabled();

  // Restore it: the editor has unsaved edits of this request, so the view asks first.
  await openHistory(page);
  await page.getByTestId("tl-entry").last().getByTestId("tl-restore").click();
  await expect(page.getByTestId("tl-confirm")).toBeVisible();
  await page.getByTestId("tl-confirm-no").click();
  await expect(page.getByTestId("tl-confirm")).toHaveCount(0);
  await page.getByTestId("tl-entry").last().getByTestId("tl-restore").click();
  await page.getByTestId("tl-confirm-yes").click();
  await expect(page.getByTestId("tl-notice")).toContainText("Restored version");
  await expect(page.getByTestId("save")).toBeDisabled();
  // The restored file differs from HEAD: it is a change to commit.
  await page.getByTestId("git-view-close").click();
  await expect(page.getByTestId("method-select")).toHaveValue("GET");
});
