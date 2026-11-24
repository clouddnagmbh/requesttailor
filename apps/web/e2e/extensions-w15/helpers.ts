// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand E: the Git extension points with the demo extension of the dev server
// (`?backend=mock&extensions=demo15`, apps/web/src/dev-extensions.ts). Mock backend only.

import { expect, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
export type Scenario = import("../../../ui/src/commands/mock-git").MockGitScenario;
export type W = Window & { __rt: { backend: MockBackend & { git: MockGit } }; __demo15?: string[] };

export const MOD = "ControlOrMeta";

/** Opens the demo collection with the demo extension and a Git scenario (the Git panel shown). */
export async function openDemo(page: Page, { scenario = "clean", staging = false }: { scenario?: Scenario; staging?: boolean } = {}): Promise<void> {
  await page.goto(`/?backend=mock&extensions=demo15${staging ? "&staging=1" : ""}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async (scenario) => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario(scenario);
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  }, scenario);
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
  await expect(page.getByTestId("git-panel")).not.toHaveAttribute("data-situation", "loading");
}

export async function palette(page: Page, label: string): Promise<void> {
  await page.keyboard.press(`${MOD}+k`);
  const box = page.getByRole("dialog", { name: "Command palette" });
  await box.getByRole("combobox").fill(label);
  await box.getByRole("option").filter({ hasText: label }).first().click();
}

/** What the demo extension's actions logged (`window.__demo15`). */
export const demoLog = (page: Page) => page.evaluate(() => (window as unknown as W).__demo15 ?? []);

/** The action of the demo extension for a kind of target, as a button. */
export const action = (page: Page, kind: string) => page.locator(`[data-testid="extension-action"][data-action="demo15/act-${kind}"]`);

export async function openChanges(page: Page): Promise<void> {
  await page.getByTestId("git-all-changes").click();
  await expect(page.getByTestId("changes-view")).toBeVisible();
  await expect(page.getByTestId("changes-row").first()).toBeVisible();
}

export async function openGraph(page: Page): Promise<void> {
  await palette(page, "Git: History");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "graph");
  await expect(page.getByTestId("graph-row").first()).toBeVisible();
}

/** A conflicted merge of health.yml (`other` and `main` both changed it). */
export async function conflictedMerge(page: Page): Promise<void> {
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
}
