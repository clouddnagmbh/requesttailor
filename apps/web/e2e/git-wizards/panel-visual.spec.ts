// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: the Git panel in every situation, light and dark, at sidebar widths 200, 260
// and 400 px: nothing overflows, blocks keep the sidebar inset. Screenshots go to `RT_SHOTS` (a
// folder) when set, else to the test's output folder, for the review by hand.

import { test } from "@playwright/test";

import { expectInsideAndAligned, openGitPanel, type Scenario } from "./helpers";

const SCENARIOS: Scenario[] = ["showcase", "clean", "none", "empty", "no-remote", "unpublished", "behind", "diverged", "detached", "conflict"];
const WIDTHS = [200, 260, 400];
const THEMES = ["light", "dark"] as const;

for (const scenario of SCENARIOS) {
  test(`panel: ${scenario}`, async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 900 });
    for (const theme of THEMES) {
      for (const width of WIDTHS) {
        await openGitPanel(page, { scenario, width, theme });
        if (scenario === "conflict") {
          await page.getByTestId("git-sync-menu").click();
          await page.getByTestId("git-sync-menu-pull").click();
          await page.getByTestId("git-merging").waitFor();
        }
        await expectInsideAndAligned(page);
        const name = `git-panel-${scenario}-${theme}-${width}.png`;
        const dir = process.env.RT_SHOTS;
        const side = page.getByTestId("git-panel").locator("xpath=ancestor::div[contains(@class,'side')][1]");
        await side.screenshot({ path: dir ? `${dir}/${name}` : test.info().outputPath(name) });
      }
    }
  });
}
