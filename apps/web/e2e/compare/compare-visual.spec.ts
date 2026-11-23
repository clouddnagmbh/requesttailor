// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand H: the graphical comparison against the mock backend (`?backend=mock`). Two
// branches: `main` asserts that the health check answers 200; `feature/pets` moves the check to
// /status/503 (green → red), edits "List users" and adds "Pets". Everything with clicks and
// keys: both sides from the picker, swap, "Compare with main", the numbers, the side-by-side
// definition and answers, the filters and n/p.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";

async function setup(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    await b.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
    const edit = async (path: string, message: string, change: (s: Awaited<ReturnType<typeof b.getRequest>>) => object) => {
      const spec = await b.getRequest(path);
      await b.updateRequest(path, { ...spec, ...change(spec) });
      await b.save();
      await b.git.commit(message, [path]);
    };
    await edit("health.yml", "Assert 200", () => ({
      assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
    }));
    await b.git.createBranch("feature/pets", true);
    await edit("health.yml", "Health moved", () => ({ url: "{{baseUrl}}/status/503" }));
    await edit("users/list-users.yml", "Second page", (s) => ({ url: "{{baseUrl}}/users?page=2", params: s.params.map((p) => ({ ...p, value: "2" })) }));
    const pets = await b.createRequest("", "Pets");
    const spec = await b.getRequest(pets);
    await b.updateRequest(pets, { ...spec, url: "{{baseUrl}}/pets" });
    await b.save();
    await b.git.commit("Add pets", [pets]);
  });
}

async function openCompare(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("compare rev");
  await dialog.getByRole("option").filter({ hasText: "Compare revisions" }).click();
  await expect(page.getByTestId("compare-panel")).toBeVisible();
}

async function pick(page: Page, side: "from" | "to", rev: string) {
  await page.getByTestId(`compare-${side}`).click();
  const list = page.getByTestId(`compare-${side}-list`);
  await expect(list).toBeVisible();
  await list.locator(`[role="option"][data-rev="${rev}"]`).first().click();
  await expect(list).toBeHidden();
  await expect(page.getByTestId(`compare-${side}`)).toHaveValue(rev);
}

const count = (page: Page, id: string) => page.getByTestId(`compare-count-${id}`).locator(".value");

test("compares two branches with clicks: numbers, side by side, filters, n and p", async ({ page }) => {
  await setup(page);
  await openCompare(page);
  await expect(page.getByTestId("compare-intro")).toBeVisible();

  // The picker lists the branches on their lanes; the current one is marked.
  await page.getByTestId("compare-from").click();
  const list = page.getByTestId("compare-from-list");
  await expect(list.getByRole("group", { name: "Branches", exact: true })).toContainText("feature/pets");
  await expect(list.getByRole("group", { name: "Branches", exact: true })).toContainText("current");
  await expect(list.getByRole("group", { name: "Recent commits" })).toContainText("Add pets");
  await expect(list.locator("svg.lanes").first()).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(list).toBeHidden();

  await pick(page, "from", "main");
  await pick(page, "to", "feature/pets");
  await page.getByTestId("compare-start").click();

  // The numbers on top.
  await expect(count(page, "changed")).toHaveText("2");
  await expect(count(page, "new")).toHaveText("1");
  await expect(count(page, "removed")).toHaveText("0");
  await expect(count(page, "failures")).toHaveText("1");
  await expect(page.getByTestId("compare-summary")).toContainText("Add pets");

  // Only the changes are listed; the first is selected and shown side by side.
  const rows = page.getByTestId("compare-row");
  await expect(rows).toHaveCount(3);
  const health = rows.filter({ hasText: "Health" });
  await expect(health).toContainText("status 200 → 503");
  await expect(health.getByTestId("compare-row-regressed")).toBeVisible();
  await health.click();
  const detail = page.getByTestId("compare-detail");
  await expect(detail).toHaveAttribute("data-path", "health.yml");
  const url = detail.getByTestId("cmp-field").filter({ hasText: "url" });
  await expect(url.getByTestId("cmp-old")).toContainText("/health");
  await expect(url.getByTestId("cmp-new").locator("mark")).toContainText("status");
  await expect(detail.getByTestId("cmp-old-side").getByTestId("cmp-status")).toHaveText("200");
  await expect(detail.getByTestId("cmp-new-side").getByTestId("cmp-status")).toHaveText("503");
  await expect(detail.getByTestId("cmp-time")).toContainText("ms");
  await expect(detail.getByTestId("cmp-tests")).toContainText("res.status eq 200");
  await expect(detail.getByTestId("cmp-tests")).toContainText("failed");
  const json = detail.getByTestId("cmp-json");
  await expect(json.locator('[data-pointer="/url"]')).toHaveAttribute("data-kind", "changed");
  await expect(json.locator('[data-pointer="/url"] mark').first()).toBeVisible();

  // n and p walk the changes.
  await page.getByTestId("compare-list").focus();
  await page.keyboard.press("n");
  const next = await detail.getAttribute("data-path");
  expect(next).not.toBe("health.yml");
  await page.keyboard.press("p");
  await expect(detail).toHaveAttribute("data-path", "health.yml");

  // Filters: failures, all, a tile, the search.
  await page.getByTestId("compare-filter-failures").click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toHaveAttribute("data-path", "health.yml");
  await page.getByTestId("compare-filter-all").click();
  expect(await rows.count()).toBeGreaterThan(3);
  await page.getByTestId("compare-count-new").click();
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("Pets");
  await expect(detail.getByTestId("cmp-old-side")).toContainText("Not sent at this revision");
  await page.getByTestId("compare-count-new").click();
  await expect(rows).toHaveCount(3);
  await page.getByTestId("compare-search").fill("users");
  await expect(rows).toHaveCount(1);
  await page.getByTestId("compare-search").fill("");

  // Swap: the new request is now removed.
  await page.getByTestId("compare-swap").click();
  await expect(page.getByTestId("compare-from")).toHaveValue("feature/pets");
  await expect(count(page, "removed")).toHaveText("1");
  await expect(count(page, "new")).toHaveText("0");

  // One click: the current branch against main.
  await page.getByTestId("compare-with-main").click();
  await expect(page.getByTestId("compare-from")).toHaveValue("main");
  await expect(page.getByTestId("compare-to")).toHaveValue("feature/pets");
  await expect(count(page, "new")).toHaveText("1");
});

test("compares the definitions without sending, then sends on request", async ({ page }) => {
  await setup(page);
  await openCompare(page);
  await page.getByTestId("compare-send").uncheck();
  await pick(page, "from", "main");
  await page.getByTestId("compare-to").fill("HEAD");
  await page.keyboard.press("Escape");
  await page.getByTestId("compare-start").click();
  await expect(count(page, "changed")).toHaveText("2");
  await expect(count(page, "failures")).toHaveText("–");
  const detail = page.getByTestId("compare-detail");
  await expect(detail.getByTestId("compare-not-run")).toBeVisible();
  await detail.getByTestId("compare-run").click();
  await expect(count(page, "failures")).toHaveText("1");
  await expect(detail.getByTestId("cmp-response")).toBeVisible();
});

test("the revision field takes a typed revision with the keyboard", async ({ page }) => {
  await setup(page);
  await openCompare(page);
  const from = page.getByTestId("compare-from");
  await from.click();
  await from.fill("HEAD~2");
  await expect(page.getByTestId("compare-from-list")).toContainText('compare "HEAD~2" as typed');
  await page.keyboard.press("Enter");
  await expect(from).toHaveValue("HEAD~2");
  await page.getByTestId("compare-to").click();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("compare-to")).toHaveValue("feature/pets");
  await page.getByTestId("compare-start").click();
  // HEAD~2 is "Health moved": two commits differ (list users, pets).
  await expect(count(page, "changed")).toHaveText("1");
  await expect(count(page, "new")).toHaveText("1");
});

test("a deep link from a pull request opens its comparison", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("review");
  await dialog.getByRole("option").filter({ hasText: "Review: pull and merge requests" }).click();
  await page.getByTestId("review-host").fill("git.example.com");
  await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
  await page.getByTestId("review-add-host").click();
  await page.getByTestId("review-token").fill("mock-token");
  await page.getByTestId("review-save-token").click();
  await page.getByTestId("review-row").first().click();
  await expect(page.getByTestId("review-title")).toBeVisible();

  await page.getByTestId("review-compare").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "compare");
  await expect(page.getByTestId("compare-to")).toHaveValue(/^(refs\/requesttailor\/review\/|[0-9a-f]{40}$)/);
  // Compared at once: the PR's requests, side by side.
  await expect(page.getByTestId("compare-result")).toBeVisible();
  await expect(page.getByTestId("compare-row").filter({ hasText: "ping" })).toHaveAttribute("data-state", "new");
});
