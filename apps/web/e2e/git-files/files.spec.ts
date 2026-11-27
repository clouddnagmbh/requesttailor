// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B: the file view: a tree of the repository, any file at HEAD or in the working
// tree, images and pointers, ignored files dimmed, the comparison with HEAD. Mock backend.

import { expect, test } from "@playwright/test";

import { openChanges } from "./helpers";

async function openFiles(page: import("@playwright/test").Page) {
  await openChanges(page);
  await page.getByTestId("changes-browse").click();
  await expect(page.getByTestId("file-view")).toBeVisible();
  await expect(page.getByTestId("tree-entry").first()).toBeVisible();
}

const entry = (page: import("@playwright/test").Page, path: string) => page.locator(`[data-testid="tree-entry"][data-path="${path}"]`);

test("the tree lists folders first, marks changed files and dims ignored ones", async ({ page }) => {
  await openFiles(page);
  const top = await page.locator('[data-testid="tree-entry"]').evaluateAll((els) => els.map((e) => e.getAttribute("data-path")));
  // The Git mock lists unchanged tracked files too (kit 667b1bf), so the unchanged /demo folder appears.
  expect(top.slice(0, 5)).toEqual(["/assets", "/data", "/demo", "/docs", "/legacy"]);
  await expect(entry(page, "/README.md")).toHaveAttribute("data-state", "modified");
  await expect(entry(page, "/build.log")).toHaveClass(/ignored/);
  await entry(page, "/assets").click();
  await expect(entry(page, "/assets/logo.png")).toBeVisible();
  await expect(entry(page, "/assets/logo.png")).toHaveAttribute("data-state", "modified");
  await entry(page, "/assets").click();
  await expect(entry(page, "/assets/logo.png")).toHaveCount(0);
});

test("a file reads in the working tree and at HEAD, and compares with HEAD", async ({ page }) => {
  await openFiles(page);
  await entry(page, "/README.md").click();
  await expect(page.getByTestId("blob-lines")).toContainText("guides folder");
  await page.getByTestId("file-rev-head").click();
  await expect(page.getByTestId("blob-lines")).toContainText("docs folder");
  await expect(page.getByTestId("blob-lines")).not.toContainText("guides folder");
  await page.getByTestId("file-rev-worktree").click();
  await page.getByTestId("file-compare-head").click();
  await expect(page.getByTestId("text-diff")).toBeVisible();
  await expect(page.locator('[data-testid="text-line"][data-kind="added"] mark').first()).toBeVisible();
});

test("an image shows itself, a pointer and a binary file their facts", async ({ page }) => {
  await openFiles(page);
  await entry(page, "/assets").click();
  await entry(page, "/assets/logo.png").click();
  await expect(page.getByTestId("blob-image")).toBeVisible();
  await entry(page, "/assets/model.glb").click();
  await expect(page.getByTestId("blob-facts")).toContainText("Git LFS pointer");
  await expect(page.getByTestId("blob-facts")).toContainText("5.0 MB");
});

test("another revision can be typed, and a deleted file stays readable at HEAD", async ({ page }) => {
  await openFiles(page);
  await page.getByTestId("file-rev-other").click();
  await page.getByTestId("file-rev-input").fill("HEAD");
  await page.getByTestId("file-rev-input").press("Enter");
  await entry(page, "/legacy").click();
  await entry(page, "/legacy/old.txt").click();
  await expect(page.getByTestId("blob-lines")).toContainText("Replaced by docs/guide.md");
  await page.getByTestId("file-rev-other").click();
  await page.getByTestId("file-rev-input").fill("no-such-branch");
  await page.getByTestId("file-rev-input").press("Enter");
  await expect(page.getByTestId("file-error")).toBeVisible();
});

test("a file chosen from outside opens it, with its folders open", async ({ page }) => {
  await openChanges(page);
  await page.getByTestId("changes-row").filter({ hasText: "logo.png" }).dblclick();
  await expect(page.getByTestId("file-view")).toBeVisible();
  await expect(page.getByTestId("blob-image")).toBeVisible();
  await expect(entry(page, "/assets/logo.png")).toHaveAttribute("aria-selected", "true");
});
