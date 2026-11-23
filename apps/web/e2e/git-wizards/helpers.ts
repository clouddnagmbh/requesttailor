// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand G: opening the Git panel on a mock scenario (apps/ui/src/commands/mock-git.ts).

import { expect, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
export type Scenario = import("../../../ui/src/commands/mock-git").MockGitScenario;
export type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

export interface OpenOptions {
  scenario?: Scenario;
  identity?: boolean;
  /** Sidebar width in px. */
  width?: number;
  theme?: "light" | "dark";
}

export async function openGitPanel(page: Page, options: OpenOptions = {}): Promise<void> {
  const { scenario = "clean", identity = true, width, theme } = options;
  await page.addInitScript(
    ({ width, theme }) => {
      try {
        if (width) localStorage.setItem("rt.shell.sidebarWidth", JSON.stringify(width));
        if (theme) localStorage.setItem("rt.shell.theme", JSON.stringify(theme));
      } catch {
        // No storage: defaults.
      }
    },
    { width, theme },
  );
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(
    async ({ scenario, identity }) => {
      const git = (window as unknown as W).__rt.backend.git;
      git.scenario(scenario);
      if (identity) await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
    },
    { scenario, identity },
  );
  // The sidebar remembers its view: show another first, so the panel mounts with the scenario.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-panel")).toBeVisible();
  await expect(page.getByTestId("git-panel")).not.toHaveAttribute("data-situation", "loading");
}

export const gitCalls = (page: Page) => page.evaluate(() => [...(window as unknown as W).__rt.backend.git.calls]);

/** Everything in the panel stays inside it: no horizontal scrolling, and each top-level block
 *  keeps the sidebar inset (`--rt-side-inset`) on both sides. */
export async function expectInsideAndAligned(page: Page): Promise<void> {
  const r = await page.getByTestId("git-panel").evaluate((panel) => {
    const inset = parseFloat(getComputedStyle(document.documentElement).getPropertyValue("--rt-side-inset"));
    const box = panel.getBoundingClientRect();
    const scrollbar = panel.offsetWidth - panel.clientWidth;
    const blocks = [...panel.children]
      .filter((el) => el instanceof HTMLElement && el.offsetParent !== null && getComputedStyle(el).display !== "inline")
      .map((el) => {
        const b = el.getBoundingClientRect();
        return { name: (el as HTMLElement).dataset.testid ?? el.className, left: b.left - box.left, right: box.right - scrollbar - b.right, wide: !el.matches(".link") };
      });
    // An element sticks out when it reaches past the panel and no ancestor inside the panel
    // clips it (ellipsis rows clip on purpose).
    const clipped = (el: HTMLElement) => {
      for (let a = el.parentElement; a && a !== panel; a = a.parentElement) {
        const o = getComputedStyle(a).overflowX;
        if (o !== "visible" && a.getBoundingClientRect().right <= box.right - scrollbar + 0.5) return true;
      }
      return false;
    };
    const overflowing = [...panel.querySelectorAll<HTMLElement>("*")]
      .filter((el) => {
        const b = el.getBoundingClientRect();
        return (
          b.width > 0 &&
          (b.right > box.right - scrollbar + 0.5 || b.left < box.left - 0.5) &&
          !el.closest('[role="menu"]') &&
          !clipped(el)
        );
      })
      .map((el) => `${el.tagName.toLowerCase()}.${el.className} ${el.dataset.testid ?? ""}`);
    return { inset, blocks, overflowing, scroll: panel.scrollWidth - panel.clientWidth };
  });
  expect(r.scroll, "no horizontal scroll").toBeLessThanOrEqual(0);
  expect(r.overflowing, "nothing sticks out of the panel").toEqual([]);
  for (const b of r.blocks) {
    expect(Math.round(b.left), `${b.name} left inset`).toBe(r.inset);
    if (b.wide) expect(Math.round(b.right), `${b.name} right inset`).toBe(r.inset);
  }
}
