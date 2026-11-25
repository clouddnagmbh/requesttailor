// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand D: the rate-limit chip in the status line and the runner's "Waiting"
// line (mock backend; `?ratelimit=1|exhausted|retry` mirrors what a server announces).

import { expect, test, type Page } from "@playwright/test";

async function openCollection(page: Page, query: string) {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function send(page: Page, query: string) {
  await openCollection(page, query);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

test("a response without limits has no chip", async ({ page }) => {
  await send(page, "");
  await expect(page.getByTestId("ratelimit-chip")).toHaveCount(0);
});

test("the chip says what is left and when it resets", async ({ page }) => {
  await send(page, "&ratelimit=1");
  const chip = page.getByTestId("ratelimit-chip");
  await expect(chip).toHaveText("47 / 60 left, resets in 12 s");
  await expect(chip).toHaveAttribute("data-tone", "ok");
  await expect(chip).toHaveAttribute("title", /60 per 60 s/);
});

test("retry-after reads as rate limited", async ({ page }) => {
  await send(page, "&ratelimit=retry");
  const chip = page.getByTestId("ratelimit-chip");
  await expect(chip).toHaveText("Rate limited, retry in 12 s");
  await expect(chip).toHaveAttribute("data-tone", "bad");
});

async function runFolder(page: Page, query: string) {
  await openCollection(page, query);
  const row = page.getByTestId("tree-folder").filter({ hasText: "users" });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await expect(page.getByTestId("runner-panel")).toBeVisible();
}

test("the runner does not wait unless switched on", async ({ page }) => {
  await runFolder(page, "&ratelimit=exhausted");
  await expect(page.getByTestId("runner-respect-limits")).not.toBeChecked();
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true");
  await expect(page.getByTestId("runner-waiting")).toHaveCount(0);
});

test("with the switch on, the runner shows the wait and then goes on", async ({ page }) => {
  await runFolder(page, "&ratelimit=exhausted");
  await page.getByTestId("runner-respect-limits").check();
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-waiting")).toContainText("(rate limit)");
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true", { timeout: 20_000 });
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-failed", "0");
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-stopped", "false");
  await expect(page.getByTestId("runner-waiting")).toHaveCount(0);
});

test("cancelling during the wait stops the run at once", async ({ page }) => {
  await runFolder(page, "&ratelimit=retry");
  await page.getByTestId("runner-respect-limits").check();
  await page.getByTestId("runner-start").click();
  // Retry-After is 12 s: the second request is held back after the first one ran.
  await expect(page.getByTestId("runner-waiting")).toContainText("Waiting 12 s (rate limit)");
  const before = Date.now();
  await page.getByTestId("runner-cancel").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-stopped", "true");
  expect(Date.now() - before).toBeLessThan(3000);
  await expect(page.getByTestId("runner-waiting")).toHaveCount(0);
});

test("a wait longer than the run may take ends it with the reason", async ({ page }) => {
  await runFolder(page, "&ratelimit=long");
  await page.getByTestId("runner-respect-limits").check();
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true");
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-stopped", "true");
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-failed", "1");
  await expect(page.getByTestId("runner-waiting")).toHaveCount(0);
  await page.getByTestId("runner-step").last().click();
  await expect(page.getByTestId("runner-step-details")).toContainText("--max-wait");
});
