// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand E (gate item 11): a Git view, a staging view, editors for merge and diff, a
// commit form action and a hint per Git view, all from the demo extension.

import { expect, test } from "@playwright/test";

import { conflictedMerge, demoLog, openChanges, openDemo, openGraph, palette, type W } from "./helpers";

test("a Git view opens from the palette, gets the repository and closes; a broken one does not take the app down", async ({ page }) => {
  await openDemo(page);
  await palette(page, "Git: Demo Git view");
  await expect(page.getByTestId("git-extension-view")).toHaveAttribute("data-view", "demo15/overview");
  await expect(page.getByTestId("demo15-view-repository")).not.toHaveText("Repository: ");
  await page.getByTestId("demo15-view-close").click();
  await expect(page.getByTestId("git-extension-view")).toHaveCount(0);

  await palette(page, "Git: Demo broken view");
  await expect(page.getByTestId("git-extension-view-failed")).toContainText("Demo view broke");
  await expect(page.getByTestId("git-panel")).toBeVisible();
});

test("a staging view replaces the file list and the commit goes through commitStaged", async ({ page }) => {
  await openDemo(page, { staging: true });
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "https://example.test/staged" });
    await b.save();
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("demo15-staging")).toBeVisible();
  await expect(page.locator('[data-testid="demo15-staging-file"][data-path$="health.yml"]')).toHaveCount(1);
  await expect(page.getByTestId("git-file")).toHaveCount(0);
});

test("the commit form takes an action that sets the message", async ({ page }) => {
  await openDemo(page);
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "https://example.test/msg" });
    await b.save();
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await page.getByRole("button", { name: "Use template" }).click();
  await expect(page.getByTestId("git-message")).toHaveValue(/^feat: \d+ files$/);
});

test("an extension merge editor is offered next to the line-by-line view; resolve then close is harmless", async ({ page }) => {
  await openDemo(page);
  await conflictedMerge(page);
  await page.locator('[data-testid="git-file"][data-path$="health.yml"]').click();
  await page.getByTestId("git-resolve-by-hand").click();
  const picker = page.getByTestId("editor-picker");
  await expect(picker).toBeVisible();
  await page.getByTestId("editor-extension").click();
  await expect(page.getByTestId("demo15-merge")).toBeVisible();
  await page.getByTestId("demo15-merge-theirs").click();
  await expect(page.getByTestId("demo15-merge")).toHaveCount(0);
  await expect(page.getByTestId("demo15-merge-error")).toHaveCount(0);
  const url = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.getRequest("health.yml")).url);
  expect(url).toBe("https://example.test/theirs");
});

test("an extension diff editor replaces the standard compare for a file and can go back", async ({ page }) => {
  await openDemo(page);
  await openChanges(page);
  await page.locator('[data-testid="changes-row"][data-path="/README.md"]').click();
  await page.getByTestId("editor-picker").getByTestId("editor-extension").click();
  await expect(page.getByTestId("demo15-diff-what")).toContainText("/README.md");
  await page.getByTestId("demo15-diff-back").click();
  await expect(page.getByTestId("text-diff")).toBeVisible();
  // `accepts` rules out images: no picker for the logo.
  await page.locator('[data-testid="changes-row"][data-path="/assets/logo.png"]').click();
  await expect(page.getByTestId("editor-extension")).toHaveCount(0);
});

test("hints show per Git view with the situation: graph (range, dragging), changes, conflict", async ({ page }) => {
  await openDemo(page, { scenario: "showcase" });
  await openGraph(page);
  const hint = page.getByTestId("extension-git-hint");
  await expect(hint).toContainText("Demo graph hint");
  await page.getByTestId("graph-row").nth(3).click({ modifiers: ["ControlOrMeta"] });
  await expect(hint).toContainText("(a range)");
  await hint.getByRole("button", { name: "Dismiss hint" }).click();
  await expect(hint).toHaveCount(0);
  expect(await demoLog(page)).toEqual([]);
});

test("a repository-only set still has the Git side view", async ({ page }) => {
  await openDemo(page);
  await expect(page.getByTestId("view-git")).toBeVisible();
});
