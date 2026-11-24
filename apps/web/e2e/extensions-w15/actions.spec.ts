// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand E (gate item 11): an action of the demo extension on every kind of target,
// in the places that show them: the repository (Git panel, sidebar menu), commits and a
// selection of commits and a branch (graph), files and the changes (Changes view, file tree,
// file history), a parked change (Git panel) and a conflict.

import { expect, test } from "@playwright/test";

import { action, demoLog, openChanges, openDemo, openGraph, type W } from "./helpers";

test("the repository: a button in the Git panel and an entry in the sidebar menu", async ({ page }) => {
  await openDemo(page);
  await expect(page.getByTestId("git-repository-actions")).toBeVisible();
  await action(page, "repository").click();
  await page.getByTestId("view-requests").click();
  await page.getByTestId("repository").first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Demo repository" }).click();
  expect((await demoLog(page)).filter((l) => l.startsWith("action:repository"))).toHaveLength(2);
});

test("a commit, two marked commits and a branch in the graph", async ({ page }) => {
  await openDemo(page, { scenario: "showcase" });
  await openGraph(page);
  const details = page.getByTestId("graph-details");
  await action(page, "commit").click();
  expect((await demoLog(page))[0]).toMatch(/^action:commit:[0-9a-f]+$/);

  // Mod+click marks a second commit: the selection is a range, `commits`, newest first.
  await page.getByTestId("graph-row").nth(3).click({ modifiers: ["ControlOrMeta"] });
  await expect(details.getByTestId("graph-comparing")).toBeVisible();
  await expect(action(page, "commit")).toHaveCount(0);
  await action(page, "commits").click();
  expect((await demoLog(page))[1]).toMatch(/^action:commits:[0-9a-f]+\+[0-9a-f]+$/);

  // The context menu has them too, and a branch's submenu the branch action.
  await page.getByTestId("graph-row").filter({ hasText: "Clearer name for the large response" }).click({ button: "right" });
  await expect(page.getByRole("menuitem", { name: /Demo commit/ })).toBeVisible();
  await page.getByRole("menuitem", { name: "main", exact: true }).hover();
  await page.getByRole("menuitem", { name: "Demo ref" }).click();
  expect((await demoLog(page)).at(-1)).toBe("action:ref:refs/heads/main");
});

test("files and the changes in the Changes view, and a file in the file tree", async ({ page }) => {
  await openDemo(page);
  await openChanges(page);
  await action(page, "changes").click();
  expect((await demoLog(page))[0]).toBe("action:changes:/README.md+/assets/logo.png+/assets/model.glb+/data/blob.bin+/legacy/old.txt+/notes/todo.md");

  await page.locator('[data-testid="changes-row"][data-path="/README.md"]').click();
  await action(page, "file").click();
  expect((await demoLog(page))[1]).toBe("action:file:/README.md");

  await page.locator('[data-testid="changes-row"][data-path="/notes/todo.md"]').click({ button: "right" });
  await page.getByRole("menuitem", { name: "Demo file" }).click();
  expect((await demoLog(page))[2]).toBe("action:file:/notes/todo.md");

  await page.getByTestId("changes-browse").click();
  await expect(page.getByTestId("file-tree")).toBeVisible();
  await page.locator('[data-testid="tree-entry"][data-kind="file"]').first().click({ button: "right" });
  await page.getByRole("menuitem", { name: "Demo file" }).click();
  expect((await demoLog(page))[3]).toMatch(/^action:file:\//);
});

test("a parked change in the Git panel", async ({ page }) => {
  await openDemo(page);
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, url: "https://example.test/parked" });
    await b.save();
    await b.git.apply({ action: "stash" });
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-stash")).toHaveCount(1);
  await action(page, "stash").click();
  expect(await demoLog(page)).toEqual(["action:stash:0"]);
});

test("a conflicted file", async ({ page }) => {
  await openDemo(page);
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const edit = async (url: string) => {
      const spec = await b.getRequest("health.yml");
      await b.updateRequest("health.yml", { ...spec, url });
      await b.save();
    };
    await b.git.apply({ action: "create-branch", name: "other", start: "HEAD", switch: true });
    await edit("https://example.test/theirs");
    await b.git.commit("theirs", ["health.yml"]);
    await b.git.apply({ action: "switch", target: "main" });
    await edit("https://example.test/ours");
    await b.git.commit("ours", ["health.yml"]);
    await b.git.apply({ action: "merge", from: "other" });
  });
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await page.locator('[data-testid="git-file"][data-path$="health.yml"]').click();
  await action(page, "conflict").click();
  expect(await demoLog(page)).toEqual(["action:conflict:health.yml"]);
});
