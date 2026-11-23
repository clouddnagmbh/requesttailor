// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand C: branch compare and bisect against the mock backend (`?backend=mock`,
// apps/ui/src/commands/mock-compare.ts). Four commits: the health check gets an assertion
// (200), then its URL moves to /status/503 (the assertion fails), then it is renamed. The
// same views run in the real web core in compare.e2e.ts.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";

/** Opens the demo collection and makes the commits; returns their ids, oldest first. */
async function setup(page: Page): Promise<string[]> {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
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
    await edit("Assert 200", () => ({
      assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
    }));
    await edit("Health moved", () => ({ url: "{{baseUrl}}/status/503" }));
    await edit("Rename health", () => ({ name: "Health!" }));
    return ids;
  });
}

async function openFromPalette(page: Page, query: string, option: string) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(query);
  await dialog.getByRole("option").filter({ hasText: option }).click();
}

test("compares two revisions from the palette: what changed, per request", async ({ page }) => {
  const ids = await setup(page);
  await openFromPalette(page, "compare rev", "Compare revisions");
  const panel = page.getByTestId("compare-panel");
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "compare");
  await expect(panel).toContainText('today\'s environment "Local"');

  await page.getByTestId("compare-from").fill(ids[1]!);
  await page.getByTestId("compare-to").fill("HEAD");
  await page.keyboard.press("Escape");
  await page.getByTestId("compare-start").click();
  await expect(page.getByTestId("compare-count-changed").locator(".value")).toHaveText("1");
  await expect(page.getByTestId("compare-summary")).toContainText(ids[1]!.slice(0, 7));
  await expect(page.getByTestId("compare-summary")).toContainText("Rename health");

  // Only the changed request is listed; its row says what differs, the detail shows both sides.
  const rows = page.getByTestId("compare-row");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("status 200 → 503");
  const detail = page.getByTestId("compare-detail");
  await expect(detail.getByTestId("cmp-new-side").getByTestId("cmp-status")).toHaveText("503");
  await expect(detail.getByTestId("cmp-json").locator('[data-pointer="/url"]')).toContainText("/status/503");
  await detail.getByTestId("cmp-all-headers").check();
  await expect(detail.getByTestId("cmp-headers")).toContainText("x-mock-name");
  const tests = detail.getByTestId("cmp-tests");
  await expect(tests).toContainText("res.status eq 200");
  await expect(tests).toContainText("passed");
  await expect(tests).toContainText("failed");

  // Everything else answers the same; "All" lists it.
  await expect(page.getByTestId("compare-same")).not.toContainText("0 unchanged");
  await page.getByTestId("compare-filter-all").click();
  expect(await rows.count()).toBeGreaterThan(1);
  await expect(page.locator('[data-testid="compare-row"][data-same="true"]').first()).toContainText("same at both revisions");
});

test("an unknown revision shows the error and keeps the form", async ({ page }) => {
  await setup(page);
  await openFromPalette(page, "compare rev", "Compare revisions");
  await page.getByTestId("compare-from").fill("nope");
  await page.keyboard.press("Escape");
  await page.getByTestId("compare-start").click();
  await expect(page.getByTestId("compare-error")).toContainText(/nope/i);
  await expect(page.getByTestId("compare-start")).toBeEnabled();
});

test("bisect finds the commit that broke the assertion and opens the comparison", async ({ page }) => {
  const ids = await setup(page);
  await openFromPalette(page, "bisect", "Bisect");
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "bisect");
  await page.getByTestId("bisect-good").fill(ids[0]!);
  await page.getByTestId("bisect-bad").fill("HEAD");
  await page.getByTestId("bisect-start").click();

  const result = page.getByTestId("bisect-result");
  await expect(result).toContainText(`First bad commit: ${ids[2]!.slice(0, 7)} Health moved`);
  await expect(result).toContainText("res.status eq 200 (got 503)");
  // Steps: the newest commit first, tested bad; the culprit's predecessor good.
  const steps = page.getByTestId("bisect-step");
  await expect(steps.first()).toHaveAttribute("data-verdict", "bad");
  await expect(steps.first()).toContainText("Rename health");
  await expect(steps.filter({ hasText: "Assert 200" })).toHaveAttribute("data-verdict", "good");
  await expect(page.getByTestId("bisect-progress")).toContainText("tested");

  await page.getByTestId("bisect-open-compare").click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "compare");
  await expect(page.getByTestId("compare-from")).toHaveValue(`${ids[2]}~1`);
  await expect(page.getByTestId("compare-count-changed").locator(".value")).toHaveText("1");
  await expect(page.getByTestId("compare-row").first()).toContainText("status 200 → 503");
});

test("bisect says so when the bad revision passes, and keeps the layout tidy", async ({ page }) => {
  const ids = await setup(page);
  await openFromPalette(page, "bisect", "Bisect");
  await page.getByTestId("bisect-good").fill(ids[0]!);
  await page.getByTestId("bisect-bad").fill(ids[1]!);
  await page.getByTestId("bisect-start").click();
  await expect(page.getByTestId("bisect-result")).toContainText("The bad revision passes");
  await expect(page.getByTestId("bisect-open-compare")).toHaveCount(0);

  // The form stays inside the view at a narrow width (no horizontal overflow).
  await page.setViewportSize({ width: 700, height: 700 });
  const box = await page.getByTestId("bisect-panel").boundingBox();
  expect(box!.x).toBeGreaterThanOrEqual(0);
  expect(box!.x + box!.width).toBeLessThanOrEqual(700);
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
