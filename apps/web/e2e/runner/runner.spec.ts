// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 7, strand D: the collection runner (mock backend). Run a folder from the sidebar and
// the collection from the header and the palette; iterations, a data file with preview,
// bail, cancel, re-run; step details; a run of 1 500 steps stays a short, windowed list.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function runFolder(page: Page, name: string) {
  const row = page.getByTestId("tree-folder").filter({ hasText: name });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await expect(page.getByTestId("runner-panel")).toBeVisible();
}

const summary = (page: Page) => page.getByTestId("runner-summary");

test("runs a folder from the sidebar, twice with iterations, and shows step details", async ({ page }) => {
  await openCollection(page);
  await runFolder(page, "users");
  await expect(page.getByTestId("runner-title")).toHaveText("Run folder: users");
  await expect(page.getByTestId("runner-environment")).toContainText("Local");

  await page.getByTestId("runner-iterations").fill("2");
  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-finished", "true");
  await expect(summary(page)).toHaveAttribute("data-requests", "4");
  await expect(summary(page)).toHaveAttribute("data-passed", "4");
  await expect(summary(page)).toHaveAttribute("data-failed", "0");
  const steps = page.getByTestId("runner-step");
  await expect(steps).toHaveCount(4);
  await expect(steps.nth(0)).toContainText("Create user");
  await expect(steps.nth(1)).toContainText("List users");
  await expect(steps.nth(2)).toContainText("#2");

  // Details of a step: its tests.
  await steps.nth(1).click();
  const details = page.getByTestId("runner-step-details");
  await expect(details).toContainText("List users");
  await expect(details.getByTestId("runner-test")).toHaveText(/status is 2xx/);
  await expect(details.getByTestId("runner-test")).toHaveAttribute("data-passed", "true");
  // Arrow keys move through the results.
  await page.getByTestId("runner-results").press("ArrowDown");
  await expect(details).toContainText("iteration 2");

  // Run again: fresh results.
  await expect(page.getByTestId("runner-start")).toHaveText("Run again");
  await page.getByTestId("runner-iterations").fill("1");
  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-requests", "2");
  await expect(steps).toHaveCount(2);

  // Opening a request shows the editor again.
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await expect(page.getByTestId("runner-panel")).toBeHidden();
  await expect(page.getByTestId("send")).toBeVisible();
});

test("a data file drives the iterations, with a preview and clear errors", async ({ page }) => {
  await openCollection(page);
  await runFolder(page, "users");

  const input = page.getByTestId("runner-data");
  await input.setInputFiles({ name: "bad.csv", mimeType: "text/csv", buffer: Buffer.from("user,role\nada\n") });
  await expect(page.getByTestId("runner-data-error")).toContainText("line 2");

  await input.setInputFiles({
    name: "users.csv",
    mimeType: "text/csv",
    buffer: Buffer.from('user,role\r\nada,admin\r\n"bob, jr",dev\r\neve,ops\r\n'),
  });
  await expect(page.getByTestId("runner-data-error")).toBeHidden();
  await expect(page.getByTestId("runner-data-name")).toHaveText("users.csv: 3 rows");
  const preview = page.getByTestId("runner-data-preview");
  await expect(preview.getByRole("columnheader")).toHaveText(["#", "user", "role"]);
  await expect(page.getByTestId("runner-data-row")).toHaveCount(3);
  await expect(page.getByTestId("runner-data-row").nth(1)).toContainText("bob, jr");
  await expect(page.getByTestId("runner-iterations")).toBeDisabled();

  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-requests", "6");
  await expect(summary(page)).toContainText("3 iterations");

  await page.getByTestId("runner-data-clear").click();
  await expect(page.getByTestId("runner-data-preview")).toBeHidden();
  await expect(page.getByTestId("runner-iterations")).toBeEnabled();
});

test("runs the collection from the header and the palette; bail stops at the first failure", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("run-collection").click();
  await expect(page.getByTestId("runner-title")).toHaveText("Run collection: Demo API");
  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-finished", "true");
  // demo/blocked.yml fails (blocked by the browser), the rest pass.
  await expect(summary(page)).toHaveAttribute("data-requests", "5");
  await expect(summary(page)).toHaveAttribute("data-failed", "1");
  const failed = page.locator('[data-testid="runner-step"][data-state="failed"]');
  await expect(failed).toHaveCount(1);
  await failed.click();
  await expect(page.getByTestId("runner-step-error")).toBeVisible();

  await page.getByTestId("runner-close").click();
  await expect(page.getByTestId("runner-panel")).toBeHidden();

  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("run coll");
  await dialog.getByRole("option").filter({ hasText: "Run collection" }).click();
  await expect(page.getByTestId("runner-panel")).toBeVisible();
  // The last results are still there.
  await expect(summary(page)).toHaveAttribute("data-requests", "5");

  await page.getByTestId("runner-bail").check();
  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-stopped", "true");
  await expect(summary(page)).toHaveAttribute("data-requests", "1");
  await expect(page.getByTestId("runner-progress")).toContainText("Stopped");
});

test("cancel stops a running run", async ({ page }) => {
  await openCollection(page);
  await runFolder(page, "users");
  await page.getByTestId("runner-iterations").fill("100");
  await page.getByTestId("runner-delay").fill("50");
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-cancel")).toBeVisible();
  await expect(page.getByTestId("runner-progress")).toContainText("Running");
  await expect.poll(async () => Number(await summary(page).getAttribute("data-requests"))).toBeGreaterThan(1);
  await page.getByTestId("runner-cancel").click();
  await expect(summary(page)).toHaveAttribute("data-stopped", "true");
  await expect(page.getByTestId("runner-cancel")).toBeHidden();
  const done = Number(await summary(page).getAttribute("data-requests"));
  expect(done).toBeLessThan(200);
});

test("1 500 steps: the list stays windowed and scrolls to the end", async ({ page }) => {
  await openCollection(page);
  await runFolder(page, "users");
  await page.getByTestId("runner-iterations").fill("750");
  await page.getByTestId("runner-start").click();
  await expect(summary(page)).toHaveAttribute("data-finished", "true", { timeout: 30_000 });
  await expect(summary(page)).toHaveAttribute("data-requests", "1500");
  const list = page.getByTestId("runner-results");
  await expect(list).toHaveAttribute("data-count", "1500");
  // Only the visible rows (plus a few) are in the DOM.
  expect(await page.getByTestId("runner-step").count()).toBeLessThan(80);

  // End jumps to the last step; its row is rendered and selected.
  await page.getByTestId("runner-step").first().click();
  await list.press("End");
  await expect(page.locator('[data-testid="runner-step"][data-index="1499"]')).toBeVisible();
  await expect(page.getByTestId("runner-step-details")).toContainText("iteration 750");

  // Scrolling stays smooth: p95 frame gap while scrolling through the list.
  const p95 = await list.evaluate(async (el) => {
    const gaps: number[] = [];
    let last = performance.now();
    el.scrollTop = 0;
    for (let i = 0; i < 90; i++) {
      await new Promise((r) => requestAnimationFrame(r));
      const now = performance.now();
      gaps.push(now - last);
      last = now;
      el.scrollTop += 480;
    }
    gaps.sort((a, b) => a - b);
    return gaps[Math.floor(gaps.length * 0.95)]!;
  });
  // Headless Chrome on a busy CI box: generous, still catches rendering thousands of rows.
  expect(p95).toBeLessThan(60);
});
