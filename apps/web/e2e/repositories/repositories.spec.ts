// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 15, strand A (repositories in the set), mock backend: the demo folder
// is one repository holding "Demo API" and "Billing API"; "Open repository" adds a folder in Git
// without a collection (a repository root). The sidebar's "Repositories" group shows status and
// follows the choice; the active repository changes with the selection and the active collection.
// The real cores are covered by crates/rt-app/tests/repositories.rs, the desktop tests and
// e2e/shells/repositories.e2e.ts.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

const MOD = "ControlOrMeta";
type W = Window & { __rt: { backend: MockBackend } };

async function open(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const repo = (page: Page, name: string) => page.getByTestId("repository").filter({ hasText: name });

async function openRepositoryTile(page: Page) {
  await page.keyboard.press(`${MOD}+n`);
  await page.getByTestId("new-filter").fill("open repo");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("new-dialog")).toBeHidden();
}

test("the demo folder is one repository with its status; a repository root joins and is chosen", async ({ page }) => {
  await open(page);
  await expect(page.getByTestId("repository")).toHaveCount(1);
  await expect(repo(page, "demo")).toHaveAttribute("aria-current", "true");
  // Status arrives after the first frame: branch of the mock Git.
  await expect(repo(page, "demo").getByTestId("repository-branch")).toHaveText("main");

  await openRepositoryTile(page);
  await expect(page.getByTestId("repository")).toHaveText([/demo/, /notes/]);
  await expect(repo(page, "notes")).toHaveAttribute("aria-current", "true");
  await expect(repo(page, "demo")).not.toHaveAttribute("aria-current", "true");
  await expect(repo(page, "notes").getByTestId("repository-changes")).toHaveText("2");
  // No collection came with it: the collection nodes and the open collection are unchanged.
  await expect(page.getByTestId("tree-collection")).toHaveText([/Demo API/, /Billing API/]);
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  const set = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.collections.current()));
  expect(set.active_repository).toBe("mock://notes");
  expect(set.repositories.map((r) => [r.id, r.collections.length])).toEqual([
    ["mock://demo", 2],
    ["mock://notes", 0],
  ]);
});

test("choosing a repository moves Git; choosing a collection brings it back", async ({ page }) => {
  await open(page);
  await openRepositoryTile(page);
  const active = () => page.evaluate(async () => (await (window as unknown as W).__rt.backend.collections.current()).active_repository);
  expect(await active()).toBe("mock://notes");

  await repo(page, "demo").click();
  await expect(repo(page, "demo")).toHaveAttribute("aria-current", "true");
  expect(await active()).toBe("mock://demo");
  await repo(page, "notes").click();
  expect(await active()).toBe("mock://notes");

  // Opening a request of a collection makes that collection's repository the active one again.
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).first().click();
  await expect(repo(page, "demo")).toHaveAttribute("aria-current", "true");
  expect(await active()).toBe("mock://demo");
});

test("the context menu opens the graph of that repository and removes a repository root", async ({ page }) => {
  await open(page);
  await openRepositoryTile(page);
  await repo(page, "notes").click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: "Graph" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Changes" })).toBeVisible();
  await page.getByRole("menuitem", { name: "Graph" }).click();
  await expect(page.getByTestId("git-graph")).toBeVisible();

  await repo(page, "notes").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Remove from sidebar" }).click();
  await expect(page.getByTestId("repository")).toHaveCount(1);
  await expect(repo(page, "demo")).toBeVisible();
});

test("a repository root alone: no collection open, the sidebar lists it and Git views work", async ({ page }) => {
  await page.goto("/?backend=mock");
  await expect(page.getByTestId("repository")).toHaveCount(0);
  await openRepositoryTile(page);
  await expect(page.getByTestId("repository")).toHaveText([/notes/]);
  await expect(repo(page, "notes")).toHaveAttribute("aria-current", "true");
  await expect(repo(page, "notes").getByTestId("repository-branch")).toHaveText("main");
  await expect(page.getByTestId("tree-collection")).toHaveCount(0);
  await expect(page.getByTestId("collection-name")).toHaveText("");
  await expect(page.getByTestId("start")).toBeVisible();
  // The Git views of the repository open without any collection.
  await repo(page, "notes").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Graph" }).click();
  await expect(page.getByTestId("git-graph")).toBeVisible();
});

test("Clone repository: the tile asks for the address and the clone joins the sidebar", async ({ page }) => {
  await open(page);
  await page.keyboard.press(`${MOD}+n`);
  await page.getByTestId("new-filter").fill("clone repo");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("extension-dialog")).toBeVisible();
  await page.getByTestId("git-clone-url").fill("https://git.example.com/acme/demo-api.git");
  await page.getByTestId("clone-submit").click();
  await expect(page.getByTestId("extension-dialog")).toBeHidden();
  const calls = await page.evaluate(() => [...(window as unknown as W & { __rt: { backend: { git: { calls: string[] } } } }).__rt.backend.git.calls]);
  expect(calls).toContain("clone https://git.example.com/acme/demo-api.git");
  await expect(page.getByTestId("repository").first()).toBeVisible();
  await expect(page.getByTestId("tree-collection").first()).toBeVisible();
});

test("a failed clone says why and keeps the dialog", async ({ page }) => {
  await open(page);
  await page.keyboard.press(`${MOD}+n`);
  await page.getByTestId("new-filter").fill("clone repo");
  await page.keyboard.press("Enter");
  // A browser cannot read a path: refused with the reason (the mock behaves like the browser).
  await page.evaluate(() => {
    (window as unknown as W & { __rt: { backend: { git: { pickRepository?: string } } } }).__rt.backend.git.pickRepository = undefined;
  });
  await page.getByTestId("git-clone-url").fill("/Users/me/origin.git");
  await page.getByTestId("clone-submit").click();
  await expect(page.getByTestId("clone-error")).toContainText("desktop app");
  await expect(page.getByTestId("extension-dialog")).toBeVisible();
});

for (const theme of ["light", "dark"] as const) {
  for (const width of [900, 1440]) {
    test(`repositories group at ${width} px, ${theme}`, async ({ page }) => {
      await page.addInitScript((t) => localStorage.setItem("rt.shell.theme", JSON.stringify(t)), theme);
      await page.setViewportSize({ width, height: 760 });
      await open(page);
      await openRepositoryTile(page);
      await expect(repo(page, "notes").getByTestId("repository-status")).toBeVisible();
      await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
      const overflow = await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
      expect(overflow).toBeLessThanOrEqual(0);
      // Rows share a height and a left edge; nothing is cut off at the right of the sidebar.
      const rows = await page.getByTestId("repository").evaluateAll((els) =>
        els.map((e) => {
          const r = e.getBoundingClientRect();
          return { x: Math.round(r.x), h: Math.round(r.height), right: Math.round(r.right) };
        }),
      );
      expect(new Set(rows.map((r) => r.x)).size).toBe(1);
      expect(new Set(rows.map((r) => r.h)).size).toBe(1);
      const side = await page.getByTestId("sidebar").boundingBox();
      for (const r of rows) expect(r.right).toBeLessThanOrEqual(Math.round(side!.x + side!.width));
      if (process.env.RT_SHOTS) await page.screenshot({ path: `${process.env.RT_SHOTS}/repositories-${width}-${theme}.png` });
    });
  }
}
