// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the wizards in light and dark, at a 900 px and a 1440 px window: the dialog
// fits, nothing sticks out. Screenshots go to `RT_SHOTS` when set, for the review by hand.

import { expect, test, type Page } from "@playwright/test";

import { branchMenu, openGitPanel, type Scenario } from "./helpers";

interface Case {
  name: string;
  scenario: Scenario;
  open: (page: Page) => Promise<void>;
}

const CASES: Case[] = [
  {
    name: "create-branch",
    scenario: "clean",
    open: async (page) => {
      await branchMenu(page, "New branch…");
      await page.getByTestId("git-wizard-name").fill("feature/pagination");
    },
  },
  {
    name: "merge",
    scenario: "behind",
    open: async (page) => {
      await branchMenu(page, "Merge into main…");
      await page.getByTestId("git-wizard-from").selectOption("origin/main");
      await expect(page.getByTestId("git-wizard-merge")).toBeVisible();
    },
  },
  {
    name: "diverged",
    scenario: "diverged",
    open: async (page) => {
      await page.getByTestId("git-diverged-theirs").click();
    },
  },
  {
    name: "discard-confirm",
    scenario: "showcase",
    open: async (page) => {
      await page.getByTestId("git-changes-menu").click();
      await page.getByRole("menuitem", { name: "Discard changes…" }).click();
      await expect(page.getByTestId("git-wizard-confirm")).toBeEnabled();
      await page.getByTestId("git-wizard-confirm").click();
    },
  },
  {
    name: "result-undo",
    scenario: "ahead",
    open: async (page) => {
      await page.getByTestId("git-commit-menu").first().click();
      await page.getByRole("menuitem", { name: "Undo this commit…" }).click();
      await expect(page.getByTestId("git-wizard-confirm")).toBeEnabled();
      await page.getByTestId("git-wizard-confirm").click();
      await expect(page.getByTestId("git-wizard-result")).toBeVisible();
    },
  },
];

for (const c of CASES) {
  test(`wizard: ${c.name}`, async ({ page }) => {
    for (const theme of ["light", "dark"] as const) {
      for (const width of [900, 1440]) {
        await page.setViewportSize({ width, height: 860 });
        await openGitPanel(page, { scenario: c.scenario, theme });
        await c.open(page);
        const dialog = page.getByTestId("extension-dialog");
        await expect(dialog).toBeVisible();
        await expect(page.getByTestId("git-wizard")).not.toHaveAttribute("data-step", "running");
        await page.waitForTimeout(250);
        const box = (await dialog.boundingBox())!;
        expect(box.x).toBeGreaterThanOrEqual(0);
        expect(box.x + box.width).toBeLessThanOrEqual(width);
        expect(await dialog.evaluate((el) => el.scrollWidth <= el.clientWidth + 1)).toBe(true);
        const name = `git-wizard-${c.name}-${theme}-${width}.png`;
        const dir = process.env.RT_SHOTS;
        await page.screenshot({ path: dir ? `${dir}/${name}` : test.info().outputPath(name) });
      }
    }
  });
}
