// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: blame, file history of any file, history search and "Lost work" against the
// mock backend, opened the way people open them: from the palette and from the graph.

import { expect, test, type Page } from "@playwright/test";

import { confirmWizard } from "../git-wizards/helpers";
import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";

async function open(page: Page): Promise<void> {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario("showcase");
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
}

async function palette(page: Page, entry: string): Promise<void> {
  await page.keyboard.press(`${MOD}+k`);
  const p = page.getByRole("dialog", { name: "Command palette" });
  await p.getByRole("combobox").fill(entry);
  await p.getByRole("option").filter({ hasText: entry }).first().click();
}

test("search finds commits by message and by content, and opens one in the graph", async ({ page }) => {
  await open(page);
  await palette(page, "Git: Search history");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "search");
  await expect(page.getByTestId("search-intro")).toBeVisible();
  await expect(page.getByTestId("search-run")).toBeDisabled();

  await page.getByTestId("search-message").fill("merge");
  await page.getByTestId("search-run").click();
  const hits = page.getByTestId("search-hit");
  await expect(hits.first()).toBeVisible();
  await expect(hits.first()).toContainText("Message");
  const count = await hits.count();
  expect(count).toBeGreaterThan(0);

  await page.getByTestId("search-message").fill("zzz-nothing-like-this");
  await page.getByTestId("search-run").click();
  await expect(page.getByTestId("search-none")).toBeVisible();

  // A content search lists the files the line was in.
  await page.getByTestId("search-message").fill("");
  await page.getByTestId("search-content").fill("example");
  await page.getByTestId("search-run").click();
  await expect(hits.first()).toBeVisible();
  await expect(hits.first()).toContainText("Content");

  await hits.first().locator("button.main").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "graph");
});

test("the graph's file list leads to the blame and the history of any file", async ({ page }) => {
  await open(page);
  await palette(page, "Git: History");
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
  await page.getByTestId("graph-file").first().click();
  await page.getByTestId("graph-file-history").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "file-history");
  await expect(page.getByTestId("file-history-entry").first()).toBeVisible();

  await page.getByTestId("file-history-blame").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "blame");
  await expect(page.getByTestId("blame-line").first()).toBeVisible();
  await expect(page.getByTestId("blame-commit").first()).toBeVisible();
});

test("Lost work lists the steps and brings a commit back as a branch", async ({ page }) => {
  await open(page);
  await palette(page, "Git: Lost work");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "reflog");
  await expect(page.getByTestId("reflog-entry").first()).toBeVisible();
  await page.getByTestId("reflog-bring-back").first().click();
  const wizard = page.getByTestId("git-wizard");
  await expect(wizard).toBeVisible();
  await expect(page.getByTestId("git-wizard-name")).toHaveValue(/^recovered-/);
  await confirmWizard(page);
  const branches = await page.evaluate(async () => (await (window as unknown as W).__rt.backend.git.refs()).map((r) => r.name));
  expect(branches.some((n) => n.startsWith("recovered-"))).toBe(true);
});
