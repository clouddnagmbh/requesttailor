// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Cloning from a local path: the field is a plain text input (no browser URL validation),
// "Choose folder…" fills it where the backend can pick a folder, the browser gets a hint,
// and the dialog is reachable without an open collection (start screen and palette).
// Mock backend; the real cores are covered by crates/rt-git and the desktop tests.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const gitCalls = (page: Page) => page.evaluate(() => [...(window as unknown as W).__rt.backend.git.calls]);
const browserLike = (page: Page) =>
  page.evaluate(() => {
    (window as unknown as W).__rt.backend.git.pickRepository = undefined;
  });

test("start screen: clone a local path without an open collection", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("start-clone").click();
  const url = page.getByTestId("git-clone-url");
  await expect(url).toHaveAttribute("type", "text");
  await expect(url).toHaveAttribute("placeholder", "https://github.com/acme/api-tests.git or a local path");
  await url.fill("~/ClaudeCode/requesttailor-git-playground-origin.git");
  await expect(page.getByTestId("clone-submit")).toBeEnabled();
  await page.getByTestId("clone-submit").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  expect(await gitCalls(page)).toContain("clone ~/ClaudeCode/requesttailor-git-playground-origin.git");
});

test("Choose folder… fills the field", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("start-clone").click();
  await page.getByTestId("git-clone-choose").click();
  await expect(page.getByTestId("git-clone-url")).toHaveValue("/Users/demo/origin/api-tests.git");
  await page.getByTestId("clone-submit").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
});

test("palette opens the clone dialog; the Git panel accepts a path too", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("Clone a repository");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("clone-dialog")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("clone-dialog")).toBeHidden();

  await page.getByTestId("open-collection").click();
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.scenario("none"));
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-clone-url").fill("/Users/me/origin.git");
  await page.getByTestId("git-clone").click();
  await expect(page.getByTestId("git-branch")).toHaveText("main");
  expect(await gitCalls(page)).toContain("clone /Users/me/origin.git");
});

test("a browser shows a hint for a local path and refuses clearly", async ({ page }) => {
  await page.goto("/?backend=mock");
  await browserLike(page);
  await page.getByTestId("start-clone").click();
  await expect(page.getByTestId("git-clone-choose")).toHaveCount(0);
  await expect(page.getByTestId("git-clone-local-hint")).toHaveCount(0);
  await page.getByTestId("git-clone-url").fill("/Users/me/origin.git");
  await expect(page.getByTestId("git-clone-local-hint")).toContainText("desktop app");
  await page.getByTestId("clone-submit").click();
  await expect(page.getByTestId("clone-error")).toContainText("desktop app");
  // HTTPS still works.
  await page.getByTestId("git-clone-url").fill("https://git.example.com/acme/demo-api.git");
  await expect(page.getByTestId("git-clone-local-hint")).toHaveCount(0);
});

test("clone dialog fits at 900 px and in both themes", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await page.goto("/?backend=mock");
  await page.getByTestId("start-clone").click();
  await page.getByTestId("git-clone-url").fill("/Users/martinkoch/ClaudeCode/requesttailor-git-playground-origin.git");
  const dialog = page.getByTestId("clone-dialog");
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    const box = await dialog.boundingBox();
    expect(box!.x).toBeGreaterThanOrEqual(0);
    expect(box!.x + box!.width).toBeLessThanOrEqual(900);
    expect(await dialog.evaluate((d) => d.scrollWidth <= d.clientWidth)).toBe(true);
    await page.screenshot({ path: process.env.RT_SHOT_DIR ? `${process.env.RT_SHOT_DIR}/clone-${scheme}.png` : undefined });
  }
});
