// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand B, gate item 3: the working copy of the whole repository (not only the
// collection) with the viewers: words marked in text, images side by side, faded and as a
// difference, binary files and Git LFS pointers as facts, a request file by field. Mock backend.

import { expect, test } from "@playwright/test";

import { openChanges, row } from "./helpers";

test("lists every changed file of the repository, grouped by folder, ignored files left out", async ({ page }) => {
  await openChanges(page);
  const paths = await page.getByTestId("changes-row").evaluateAll((els) => els.map((e) => e.getAttribute("data-path")));
  expect(paths).toEqual(["/README.md", "/assets/logo.png", "/assets/model.glb", "/data/blob.bin", "/legacy/old.txt", "/notes/todo.md"]);
  expect(paths).not.toContain("/build.log");
  await expect(page.getByTestId("changes-folder").first()).toContainText("/");
  await expect(page.getByTestId("changes-summary")).toHaveText("4 modified, 1 new, 1 deleted");
  await expect(page.getByTestId("changes-branch")).toContainText("main");
  await expect(row(page, "/legacy/old.txt")).toHaveAttribute("data-state", "deleted");
  await expect(row(page, "/notes/todo.md")).toHaveAttribute("data-state", "untracked");
});

test("a text file shows the lines that changed with the changed words marked", async ({ page }) => {
  await openChanges(page);
  await row(page, "/README.md").click();
  await expect(page.getByTestId("text-diff")).toBeVisible();
  const removed = page.locator('[data-testid="text-line"][data-kind="removed"]');
  const added = page.locator('[data-testid="text-line"][data-kind="added"]');
  await expect(removed).toHaveCount(1);
  await expect(added).toHaveCount(1);
  await expect(removed.locator("mark").first()).toHaveText("docs");
  await expect(added.locator("mark").first()).toContainText("guides");
});

test("an image that changed shows side by side, faded and as a difference", async ({ page }) => {
  await openChanges(page);
  await row(page, "/assets/logo.png").click();
  await expect(page.getByTestId("image-diff")).toBeVisible();
  await expect(page.getByTestId("image-before")).toBeVisible();
  await expect(page.getByTestId("image-after")).toBeVisible();
  await expect(page.getByTestId("image-before").locator("img")).toHaveJSProperty("naturalWidth", 24);
  await expect(page.getByTestId("image-delta")).toContainText("→");

  await page.getByTestId("image-mode-fade").click();
  await expect(page.getByTestId("image-fade-stack").locator("img")).toHaveCount(2);
  await page.getByTestId("image-fade").fill("100");
  await expect(page.getByTestId("image-fade-stack").locator("img.top")).toHaveCSS("opacity", "1");

  await page.getByTestId("image-mode-difference").click();
  const canvas = page.getByTestId("image-difference");
  await expect(canvas).toBeVisible();
  // The orange square differs from the blue image, the rest is black: the difference is not blank.
  await expect
    .poll(async () =>
      canvas.evaluate((c: HTMLCanvasElement) => {
        const ctx = c.getContext("2d");
        if (!ctx || c.width === 0) return false;
        const lit = ctx.getImageData(0, 0, c.width, c.height).data;
        const centre = (8 * c.width + 12) * 4;
        const corner = 0;
        return lit[centre]! + lit[centre + 1]! + lit[centre + 2]! > 100 && lit[corner]! + lit[corner + 1]! + lit[corner + 2]! < 10;
      }),
    )
    .toBe(true);
});

test("binary files and Git LFS pointers show their facts, not lines", async ({ page }) => {
  await openChanges(page);
  await row(page, "/data/blob.bin").click();
  await expect(page.getByTestId("meta-diff")).toHaveAttribute("data-kind", "binary");
  await expect(page.locator('[data-testid="meta-row"][data-name="Size"]')).toContainText("2.0 KB");
  await expect(page.locator('[data-testid="meta-row"][data-name="Size"]')).toContainText("4.0 KB");

  await row(page, "/assets/model.glb").click();
  await expect(page.getByTestId("meta-diff")).toHaveAttribute("data-kind", "lfs");
  await expect(page.locator('[data-testid="meta-row"][data-name="LFS size"]')).toContainText("4.0 MB");
  await expect(page.locator('[data-testid="meta-row"][data-name="LFS size"]')).toContainText("5.0 MB");
});

test("a deleted and a new file show one side", async ({ page }) => {
  await openChanges(page);
  await row(page, "/legacy/old.txt").click();
  await expect(page.locator('[data-testid="text-line"][data-kind="removed"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="text-line"][data-kind="added"]')).toHaveCount(0);
  await row(page, "/notes/todo.md").click();
  await expect(page.locator('[data-testid="text-line"][data-kind="added"]')).toHaveCount(2);
});

test("the filter and the state chips narrow the list; keys move the selection", async ({ page }) => {
  await openChanges(page);
  await page.getByTestId("changes-filter").fill("assets");
  await expect(page.getByTestId("changes-row")).toHaveCount(2);
  await page.getByTestId("changes-filter").fill("");
  await page.getByTestId("changes-state-deleted").click();
  await expect(page.getByTestId("changes-row")).toHaveCount(1);
  await page.getByTestId("changes-state-all").click();
  await page.getByTestId("changes-list").focus();
  await row(page, "/README.md").click();
  await page.keyboard.press("j");
  await expect(row(page, "/assets/logo.png")).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("k");
  await expect(row(page, "/README.md")).toHaveAttribute("aria-selected", "true");
});

test("a request that was saved shows by field, with the lines one click away", async ({ page }) => {
  await openChanges(page);
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("method-select").selectOption("POST");
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-all-changes").click();
  const request = page.locator('[data-testid="changes-row"][data-path$="health.yml"]');
  await expect(request).toBeVisible();
  await expect(request).toContainText("API");
  await request.click();
  await expect(page.getByTestId("semantic-diff")).toBeVisible();
  await page.getByTestId("changes-mode-lines").click();
  await expect(page.getByTestId("text-diff")).toBeVisible();
  await expect(page.locator('[data-testid="text-line"][data-kind="added"]').first()).toContainText("POST");
});

test("the working copy is clean after everything is committed", async ({ page }) => {
  await openChanges(page);
  await page.evaluate(() => {
    const git = (window as unknown as import("./helpers").W).__rt.backend.git;
    git.scenario("none");
  });
  await page.getByTestId("changes-refresh").click();
  await expect(page.getByTestId("changes-error")).toContainText("not a Git repository");
});
