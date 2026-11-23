// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand A in the browser with the real core (rt-wasm, OPFS folders): a folder with a
// collection opens, "Open repository" adds a second folder in Git without a collection as a
// repository root; both are listed under "Repositories", the choice moves the active repository,
// and a reload brings both back.

import { expect, test, type Page } from "@playwright/test";

import { fillFolder } from "./env";

const manifest = 'opencollection: "1.0.0"\ninfo:\n  name: Work API\n';
const GIT = { ".git/HEAD": "ref: refs/heads/main\n" };

const repo = (page: Page, name: string) => page.getByTestId("repository").filter({ hasText: name });

async function pickerFor(page: Page) {
  await page.addInitScript(() => {
    (window as unknown as { __next: string }).__next = "work";
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () =>
        (await navigator.storage.getDirectory()).getDirectoryHandle((window as unknown as { __next: string }).__next, { create: true }),
    });
  });
}

test("a folder in Git without a collection joins as a repository root and survives a reload", async ({ page }) => {
  await pickerFor(page);
  await page.goto("/");
  await fillFolder(page, { ...GIT, "opencollection.yml": manifest }, "work");
  await fillFolder(page, { ...GIT, "todo.md": "- ship\n" }, "notes");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Work API");
  await expect(page.getByTestId("repository")).toHaveCount(1);

  await page.evaluate(() => ((window as unknown as { __next: string }).__next = "notes"));
  await page.keyboard.press("ControlOrMeta+n");
  await page.getByTestId("new-filter").fill("open repo");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("repository")).toHaveCount(2);
  await expect(repo(page, "notes")).toHaveAttribute("aria-current", "true");
  // The collection is untouched; no collection was invented for the repository.
  await expect(page.getByTestId("tree-collection")).toHaveCount(1);
  await expect(page.getByTestId("collection-name")).toHaveText("Work API");

  await repo(page, "work").click();
  await expect(repo(page, "work")).toHaveAttribute("aria-current", "true");

  await page.reload();
  await expect(page.getByTestId("repository")).toHaveCount(2);
  await expect(page.getByTestId("collection-name")).toHaveText("Work API");
});
