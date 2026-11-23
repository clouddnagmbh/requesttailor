// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand F: the commit graph ("Git: History") against the mock backend: the lanes and
// labels, the details with the changed files and their request or line diff, the context menus
// that open the wizards of strand G with a preset and the compare view, search and the keyboard.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type Scenario = import("../../../ui/src/commands/mock-git").MockGitScenario;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";

async function openGraph(page: Page, { scenario = "showcase", query = "" }: { scenario?: Scenario; query?: string } = {}): Promise<void> {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async (scenario) => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario(scenario);
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  }, scenario);
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Git: History");
  await palette.getByRole("option").filter({ hasText: "Git: History" }).first().click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "graph");
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
}

const row = (page: Page, summary: string) => page.getByTestId("graph-row").filter({ hasText: summary });
const selectedRow = (page: Page) => page.locator('[data-testid="graph-row"][aria-selected="true"]');

test("the graph shows every branch, tag and merge, HEAD selected", async ({ page }) => {
  await openGraph(page);
  await expect(page.getByTestId("graph-row")).toHaveCount(8);
  await expect(page.getByTestId("graph-count")).toHaveText("8 commits");
  // Newest first; HEAD (main, two commits to send) is selected.
  await expect(page.getByTestId("graph-summary").first()).toHaveText("Clearer name for the large response");
  await expect(selectedRow(page)).toContainText("Clearer name for the large response");
  const head = row(page, "Clearer name for the large response");
  await expect(head.locator('[data-testid="graph-ref"][data-kind="local"]')).toContainText("main");
  await expect(head.getByTestId("graph-ref-sync")).toHaveText("↑2");
  // The merge carries the remote branch and the tag; feature/auth is in step with its remote.
  const merge = row(page, "Merge feature/auth into main");
  await expect(merge.locator('[data-kind="remote"]')).toContainText("origin/main");
  await expect(merge.locator('[data-kind="tag"]')).toContainText("v1.0");
  await expect(row(page, "Move user creation to v2").getByTestId("graph-ref-tracking")).toBeVisible();
  // Lanes: the merge draws into a second lane; every row has its dot.
  await expect(page.locator('[data-testid="graph-row"] circle.dot')).toHaveCount(8);
  await expect(merge.locator("path")).not.toHaveCount(0);
  // Relative date with the absolute one as a tooltip.
  await head.getByTestId("graph-date").hover();
  await expect(page.getByRole("tooltip")).toContainText(/\d{4}/);
});

test("selecting a commit shows its files and the request diff, Lines the line diff", async ({ page }) => {
  await openGraph(page);
  await row(page, "Move user creation to v2").click();
  await expect(page.getByTestId("graph-details-summary")).toHaveText("Move user creation to v2");
  await expect(page.getByTestId("graph-file")).toHaveCount(1);
  await expect(page.getByTestId("graph-file")).toContainText("users/create-user.yml");
  await expect(page.getByTestId("graph-file").locator(".rt-badge")).toHaveText("M");
  // A file of the active collection: what changed in the request.
  await expect(page.getByTestId("graph-semantic-diff")).toContainText("Create user");
  await expect(page.getByTestId("graph-semantic-diff")).toContainText("/v2/users");
  // The same file as lines.
  await page.getByTestId("graph-diff-mode").getByRole("radio", { name: "Lines" }).click();
  await expect(page.getByTestId("graph-line-diff")).toContainText("v2/users");
  await expect(page.getByTestId("graph-line-diff").locator('[data-kind="added"]').first()).toBeVisible();
  // Parents are links: the merge has two.
  await row(page, "Merge feature/auth into main").click();
  await expect(page.getByTestId("graph-parent")).toHaveCount(2);
  await page.getByTestId("graph-parent").nth(1).click();
  await expect(selectedRow(page)).toContainText("Move user creation to v2");
});

test("files outside any collection get a line diff; binary files say so", async ({ page }) => {
  await openGraph(page, { scenario: "clean", query: "&graph=300" });
  await expect(page.getByTestId("graph-count")).toHaveText("300 commits");
  // The synthetic repository's files are no collection's: README.md gets lines.
  await page.getByTestId("graph-row").nth(3).click();
  await page.getByTestId("graph-file").filter({ hasText: "README.md" }).click();
  await expect(page.getByTestId("graph-diff-mode")).toHaveCount(0);
  await expect(page.getByTestId("graph-line-diff").getByTestId("graph-hunk")).not.toHaveCount(0);
  const binary = page.getByTestId("graph-file").filter({ hasText: "logo.png" });
  if ((await binary.count()) > 0) {
    await binary.click();
    await expect(page.getByTestId("graph-line-diff")).toContainText("Binary file");
  }
});

test("Branch from here opens the branch wizard with the commit as its start", async ({ page }) => {
  await openGraph(page);
  const target = row(page, "Use the new health URL");
  const oid = await target.getAttribute("data-oid");
  await target.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Branch from here…" }).click();
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard).toBeVisible();
  await page.getByTestId("git-wizard-name").fill("hotfix");
  await page.getByTestId("git-wizard-confirm").click();
  await expect(page.getByTestId("git-wizard-done").or(page.getByTestId("git-wizard-undo")).first()).toBeVisible();
  // The branch starts at the chosen commit, and the graph shows it there after the wizard.
  const refs = await page.evaluate(() => (window as unknown as W).__rt.backend.git.refs());
  expect(refs.find((r) => r.name === "hotfix")?.target).toBe(oid);
  await page.keyboard.press("Escape");
  await expect(wizard).toBeHidden();
  await expect(row(page, "Use the new health URL").locator('[data-testid="graph-ref"]')).toContainText("hotfix");
});

test("Compare with… opens the compare view from the commit", async ({ page }) => {
  await openGraph(page);
  const target = row(page, "Send JSON when creating users");
  const oid = (await target.getAttribute("data-oid"))!;
  await target.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Compare with…" }).click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "compare");
  await expect(page.getByTestId("compare-from")).toHaveValue(new RegExp(oid.slice(0, 7)));
});

test("the keyboard: move, search, mark, menu", async ({ page }) => {
  await openGraph(page);
  const list = page.getByTestId("graph-list");
  await list.focus();
  await page.keyboard.press("ArrowDown");
  await expect(selectedRow(page)).toContainText("Bigger pages for users");
  await page.keyboard.press("End");
  await expect(selectedRow(page)).toContainText("Initial collection");
  await page.keyboard.press("Home");
  await expect(selectedRow(page)).toContainText("Clearer name for the large response");
  await page.keyboard.press("j");
  await page.keyboard.press("j");
  await expect(selectedRow(page)).toContainText("Merge feature/auth into main");
  await page.keyboard.press("k");
  await expect(selectedRow(page)).toContainText("Bigger pages for users");
  await expect(list).toHaveAttribute("aria-activedescendant", /^graph-row-/);

  // `/` searches; Enter jumps to the next match, Escape goes back to the list.
  await page.keyboard.press("/");
  await expect(page.getByTestId("graph-search")).toBeFocused();
  await page.keyboard.type("linus");
  await expect(page.getByTestId("graph-search-status")).toHaveText("1 of 2");
  await expect(selectedRow(page)).toContainText("Move user creation to v2");
  await page.keyboard.press("Enter");
  await expect(selectedRow(page)).toContainText("Bearer token for listing users");
  await page.keyboard.press("Shift+Enter");
  await expect(selectedRow(page)).toContainText("Move user creation to v2");
  await page.keyboard.press("Escape");
  await expect(list).toBeFocused();
  await expect(page.getByTestId("graph-search")).toHaveValue("");

  // Space compares with the parent; Escape stops.
  await page.keyboard.press(" ");
  await expect(page.getByTestId("graph-comparing")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("graph-comparing")).toBeHidden();
  await expect(page.getByTestId("git-view")).toBeVisible();

  // Enter goes to the changed files.
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("graph-file").first()).toBeFocused();

  // Shift+F10 opens the menu of the selected commit; Escape closes it again.
  await list.focus();
  await page.keyboard.press("Shift+F10");
  await expect(page.getByRole("menu")).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Branch from here…" })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByRole("menu")).toBeHidden();
});

test("Mod+click compares two commits", async ({ page }) => {
  await openGraph(page);
  await row(page, "Bigger pages for users").click();
  await row(page, "Use the new health URL").click({ modifiers: ["ControlOrMeta"] });
  await expect(page.getByTestId("graph-comparing")).toBeVisible();
  await expect(page.getByTestId("graph-details-summary")).toHaveText("Bigger pages for users");
  // Two commits between: the files of both, the merge's included.
  await expect(page.getByTestId("graph-file")).not.toHaveCount(0);
  await page.getByTestId("graph-open-compare").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "compare");
});

test("an empty repository and a folder without Git say so", async ({ page }) => {
  await openGraph(page, { scenario: "showcase" });
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.scenario("empty"));
  await page.getByTestId("graph-reload").click();
  await expect(page.getByTestId("graph-empty")).toContainText("No commits yet");
  await page.evaluate(() => (window as unknown as W).__rt.backend.git.scenario("none"));
  await page.getByTestId("graph-reload").click();
  await expect(page.getByTestId("graph-empty")).toContainText("No history to show");
});
