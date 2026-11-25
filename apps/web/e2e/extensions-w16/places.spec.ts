// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand C: the places extensions add actions to (response, run, bisect, the commit
// graph, the comparison and the diff of two revisions) and the assistant panel, with the demo16
// extension against the mock backend.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

import { demoLog, MOD, openCollection, openRequest, runCommand } from "./helpers";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

/** Commits that change `health.yml` (as the compare spec does); oldest first. */
async function makeCommits(page: Page): Promise<string[]> {
  return page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    await b.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
    const ids = [await b.git.revParse("HEAD")];
    const edit = async (message: string, change: (s: Awaited<ReturnType<typeof b.getRequest>>) => object) => {
      const spec = await b.getRequest("health.yml");
      await b.updateRequest("health.yml", { ...spec, ...change(spec) });
      await b.save();
      ids.push((await b.git.commit(message, ["health.yml"])).oid);
    };
    await edit("Assert 200", () => ({ assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }] }));
    await edit("Health moved", () => ({ url: "{{baseUrl}}/status/503" }));
    return ids;
  });
}

test("a response action shows for a response; failures get the inline one", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  const actions = page.getByTestId("response-status").locator("xpath=ancestor::div[contains(@class,'summary')]").getByTestId("extension-actions");
  await expect(actions).toBeVisible();
  // Not failed: the action is in the More menu.
  await expect(actions.getByTestId("extension-action")).toHaveCount(0);
  await actions.getByTestId("extension-actions-menu").click();
  await page.getByRole("menuitem", { name: "Explain the response" }).click();
  expect(await demoLog(page)).toContain("response:users/list-users.yml:false");
});

test("a response with a failing assertion has the inline action", async ({ page }) => {
  await openCollection(page);
  await page.evaluate(async () => {
    const backend = (window as unknown as W).__rt.backend;
    const spec = await backend.getRequest("users/list-users.yml");
    await backend.updateRequest("users/list-users.yml", {
      ...spec,
      assertions: [{ expression: "res.status", operator: "eq", value: "500", description: null, enabled: true }],
    });
  });
  await openRequest(page);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await page.getByTestId("extension-action").filter({ hasText: "Explain the failure" }).click();
  expect(await demoLog(page)).toContain("response:users/list-users.yml:true");
});

test("run actions show on a finished run", async ({ page }) => {
  await openCollection(page);
  const row = page.getByTestId("tree-folder").filter({ hasText: "users" });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await expect(page.getByTestId("runner-panel")).toBeVisible();
  await expect(page.getByTestId("runner-summary")).toHaveCount(0);
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true");
  await page.getByTestId("runner-summary").getByRole("button", { name: "Summarise the run" }).click();
  expect(await demoLog(page)).toContain("run:run");
});

test("bisect, the graph and the comparison offer their actions", async ({ page }) => {
  await openCollection(page);
  const ids = await makeCommits(page);

  // Compare: both revisions in the header line.
  await runCommand(page, "compare revisions");
  await page.getByTestId("compare-from").fill(ids[1]!);
  await page.getByTestId("compare-to").fill("HEAD");
  await page.keyboard.press("Escape");
  await page.getByTestId("compare-start").click();
  await expect(page.getByTestId("compare-count-changed")).toBeVisible();
  await page.getByTestId("compare-summary").getByRole("button", { name: "Release notes" }).click();
  expect(await demoLog(page)).toContain(`range:compare:${ids[1]}..HEAD`);

  // Bisect: after the result.
  await runCommand(page, "bisect");
  await page.getByTestId("bisect-good").fill(ids[0]!);
  await page.getByTestId("bisect-bad").fill("HEAD");
  await page.getByTestId("bisect-start").click();
  await expect(page.getByTestId("bisect-result")).toBeVisible();
  await page.getByTestId("bisect-result").getByRole("button", { name: "Why did it break?" }).click();
  expect(await demoLog(page)).toContain("run:bisect");

  // Graph: two commits selected together (Cmd/Ctrl-click) show the range action.
  await runCommand(page, "Git: History");
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
  await expect(page.getByRole("button", { name: "Release notes" })).toHaveCount(0);
  await page.getByTestId("graph-row").nth(0).click();
  await page.getByTestId("graph-row").nth(2).click({ modifiers: [process.platform === "darwin" ? "Meta" : "Control"] });
  await page.getByRole("button", { name: "Release notes" }).click();
  expect((await demoLog(page)).some((l) => l.startsWith("range:graph:"))).toBe(true);
});

test("the assistant panel opens from the header, the palette and the keyboard, and follows the tab", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  const panel = page.getByTestId("assistant-panel");
  await expect(panel).toHaveCount(0);

  await page.getByTestId("assistant-toggle").click();
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("demo16-focus")).toHaveText("users/list-users.yml");
  // Focus follows the active tab.
  await openRequest(page, "Create user");
  await expect(page.getByTestId("demo16-focus")).toHaveText("users/create-user.yml");
  // Escape inside the panel closes it.
  await page.getByTestId("demo16-ask").focus();
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);

  await page.keyboard.press(`${MOD}+j`);
  await expect(panel).toBeVisible();
  await page.keyboard.press(`${MOD}+j`);
  await expect(panel).toHaveCount(0);

  await runCommand(page, "Assistant");
  await expect(panel).toBeVisible();
  await page.getByTestId("assistant-close").click();
  await expect(panel).toHaveCount(0);
});

test("the diff of two revisions offers the range action", async ({ page }) => {
  await openCollection(page);
  await page.evaluate(async () => (window as unknown as W).__rt.backend.git.scenario("showcase"));
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-branch").click();
  await page.getByRole("menuitem", { name: "Compare with" }).click();
  await page.getByRole("menu").last().getByRole("menuitem").first().click();
  const view = page.getByTestId("semantic-diff-view");
  await expect(view).toHaveAttribute("data-scope", "between");
  await view.getByRole("button", { name: "Release notes" }).click();
  expect((await demoLog(page)).some((l) => l.startsWith("range:diff:"))).toBe(true);
});
