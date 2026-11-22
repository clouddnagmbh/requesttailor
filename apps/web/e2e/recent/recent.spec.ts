// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand C: the app reopens the last collection and its last active request at start,
// so a request goes out with one keystroke (budget: under three keystrokes from app start).
// Mock backend (its recent list lives in localStorage, so a reload is a restart); the same
// flow against the real web backend is in ./recent.e2e.ts.

import { expect, test, type Page } from "@playwright/test";

import { writePerfResult } from "../perf/frames";

const MOD = "ControlOrMeta";

const tab = (page: Page, name: string) => page.getByTestId("tab").filter({ hasText: name });

test("keystroke budget: after a restart one keystroke sends the last request", async ({ page }) => {
  // Earlier session: open the collection and a request.
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");

  // Restart. Nothing but the keyboard from here on, and the only key is Mod+Enter.
  await page.reload();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
  let keystrokes = 0;
  keystrokes++;
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  expect(keystrokes).toBe(1);
  test.info().annotations.push({ type: "keystrokes (restart)", description: String(keystrokes) });
  writePerfResult("keystrokes-mock", { send_keystrokes: keystrokes });
});

test("the start screen lists recent collections; one click reopens one", async ({ page }) => {
  // A list whose most recent entry cannot be reopened (gone): the start screen shows it all.
  await page.addInitScript(() => {
    if (sessionStorage.getItem("seeded")) return;
    sessionStorage.setItem("seeded", "1");
    localStorage.setItem(
      "rt.mock.recent",
      JSON.stringify([
        { id: "gone", name: "Gone API", location: "mock://gone", openedAt: 2 },
        { id: "mock-collection", name: "Demo API", location: "mock://demo", openedAt: 1 },
      ]),
    );
  });
  await page.goto("/?backend=mock");
  const items = page.getByTestId("recent-collection");
  await expect(items).toHaveCount(2);
  await expect(items.first()).toContainText("Gone API");
  await expect(items.nth(1)).toContainText("mock://demo");
  await expect(page.getByTestId("collection-name")).toHaveText("");

  await items.first().click();
  await expect(page.getByTestId("recent-notice")).toContainText("could not be opened");
  await expect(page.getByTestId("collection-name")).toHaveText("");

  await items.filter({ hasText: "Demo API" }).click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
  await expect(page.getByTestId("recent-collections")).toHaveCount(0);
});

test("a first start shows no recent list and opens nothing", async ({ page }) => {
  await page.goto("/?backend=mock");
  await expect(page.getByTestId("start")).toBeVisible();
  // Give the start-up reopen time to run: it must not open anything.
  await page.waitForTimeout(300);
  await expect(page.getByTestId("collection-name")).toHaveText("");
  await expect(page.getByTestId("recent-collections")).toHaveCount(0);
});
