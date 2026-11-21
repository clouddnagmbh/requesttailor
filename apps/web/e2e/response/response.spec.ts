// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 3, strand C (response view), against the mock backend.
// Gate point: "a 100 MB response stays scrollable".

import { expect, test, type Locator, type Page } from "@playwright/test";

const LAST_LINE = '{"line": 001638399, ';
const MAX_ROWS = 300;

async function send(page: Page, request: string) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: request }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

const rows = (page: Page) => page.getByTestId("response-view").getByTestId("response-line");

/** The row is rendered and lies completely inside the list's viewport. */
async function expectFullyVisible(viewport: Locator, row: Locator) {
  await expect(row).toBeVisible();
  const v = (await viewport.boundingBox())!;
  const r = (await row.boundingBox())!;
  expect(r.y).toBeGreaterThanOrEqual(v.y - 0.5);
  expect(r.y + r.height).toBeLessThanOrEqual(v.y + v.height + 0.5);
}

async function rowCount(page: Page) {
  return rows(page).count();
}

test.describe("100 MB response", () => {
  test.beforeEach(async ({ page }) => {
    await send(page, "100 MB response");
  });

  test("shows status, size, time and HTTP version", async ({ page }) => {
    await expect(page.getByTestId("response-size")).toHaveText("100 MB");
    await expect(page.getByTestId("response-time")).toHaveText("14 ms");
    await expect(page.getByTestId("response-version")).toHaveText("HTTP/1.1");
    await expect(page.getByTestId("response-status")).toHaveAttribute("data-class", "ok");
    // Too large for the pretty view: raw lines are shown.
    await expect(page.getByTestId("response-mode-raw")).toHaveAttribute("aria-pressed", "true");
    await expect(page.getByTestId("response-mode-pretty")).toBeDisabled();
    await expect(page.getByTestId("response-mode-tree")).toBeDisabled();
    await expect(rows(page).first()).toContainText('{"line": 000000000, ');
  });

  test("keyboard End reaches the last line", async ({ page }) => {
    const viewport = page.getByTestId("response-lines");
    await viewport.focus();
    await page.keyboard.press("End");
    await expectFullyVisible(viewport, rows(page).filter({ hasText: LAST_LINE }));
    expect(await rowCount(page)).toBeLessThan(MAX_ROWS);
    await page.keyboard.press("Home");
    await expect(rows(page).first()).toContainText('{"line": 000000000, ');
    await page.keyboard.press("PageDown");
    await page.keyboard.press("ArrowDown");
    await expect(rows(page).filter({ hasText: '{"line": 000000000, ' })).toHaveCount(0);
  });

  test("wheel scrolling stays bounded and reaches the end", async ({ page }) => {
    const viewport = page.getByTestId("response-lines");
    await viewport.hover();
    for (let i = 0; i < 20; i++) {
      await page.mouse.wheel(0, 2_000_000);
      expect(await rowCount(page)).toBeLessThan(MAX_ROWS);
    }
    await page.mouse.wheel(0, 1e9);
    await expectFullyVisible(viewport, rows(page).filter({ hasText: LAST_LINE }));
    expect(await rowCount(page)).toBeLessThan(MAX_ROWS);
    // Small steps move by pixels (three rows of 18 px here), not by pages.
    await page.mouse.wheel(0, -18 * 3);
    await expect(rows(page).filter({ hasText: LAST_LINE })).toHaveCount(0);
    await expectFullyVisible(viewport, rows(page).filter({ hasText: '{"line": 001638396, ' }));
  });

  test("dragging the scrollbar thumb to the bottom reaches the end", async ({ page }) => {
    const viewport = page.getByTestId("response-lines");
    const thumb = page.getByTestId("response-thumb");
    const track = page.getByTestId("response-scrollbar");
    const t = (await thumb.boundingBox())!;
    const tr = (await track.boundingBox())!;
    await page.mouse.move(t.x + t.width / 2, t.y + t.height / 2);
    await page.mouse.down();
    // Move in steps, checking the DOM stays bounded while scrolling.
    for (let k = 1; k <= 10; k++) {
      await page.mouse.move(t.x + t.width / 2, t.y + t.height / 2 + (tr.height * k) / 10);
      expect(await rowCount(page)).toBeLessThan(MAX_ROWS);
    }
    await page.mouse.move(t.x + t.width / 2, tr.y + tr.height + 200);
    await page.mouse.up();
    await expectFullyVisible(viewport, rows(page).filter({ hasText: LAST_LINE }));
    const after = (await thumb.boundingBox())!;
    expect(after.y + after.height).toBeCloseTo(tr.y + tr.height, 0);
  });

  test("filtering a body that is too large shows the error inline", async ({ page }) => {
    await page.getByTestId("response-filter").fill("$.line");
    await page.getByTestId("response-filter").press("Enter");
    await expect(page.getByTestId("response-filter-error")).toBeVisible();
  });
});

test.describe("JSON response", () => {
  test.beforeEach(async ({ page }) => {
    await send(page, "List users");
  });

  test("pretty view is the default for small JSON", async ({ page }) => {
    await expect(page.getByTestId("response-mode-pretty")).toHaveAttribute("aria-pressed", "true");
    await expect(rows(page).first()).toHaveText(/^\s*1\s*\{$/);
    await expect(page.getByTestId("response-lines")).toContainText('"method": "GET"');
  });

  test("headers table", async ({ page }) => {
    await page.getByTestId("response-tab-headers").click();
    const table = page.getByTestId("response-headers");
    await expect(table.getByRole("row").filter({ hasText: "content-type" })).toContainText("application/json");
    // The request as sent is listed too.
    await expect(page.getByTestId("request-headers")).toContainText("Accept");
  });

  test("timing waterfall shows dns, connect, ttfb and hides tls", async ({ page }) => {
    await page.getByTestId("response-tab-timing").click();
    const timing = page.getByTestId("response-timing");
    for (const phase of ["dns", "connect", "ttfb", "download"]) {
      await expect(timing.locator(`[data-phase="${phase}"]`)).toBeVisible();
    }
    await expect(timing.locator('[data-phase="tls"]')).toHaveCount(0);
    await expect(timing.locator('[data-phase="dns"]')).toContainText("1.2 ms");
    await expect(timing.locator('[data-phase="ttfb"]')).toContainText("12.5 ms");
    await expect(page.getByTestId("response-timing-hidden")).toContainText("TLS");
    await expect(page.getByTestId("response-timing-hidden")).toContainText("not visible in the browser");
    await expect(timing).toContainText("14 ms");
  });

  test("JSON tree expands and collapses", async ({ page }) => {
    await page.getByTestId("response-mode-tree").click();
    const tree = page.getByTestId("response-tree");
    const headers = tree.locator('[data-path="$.headers"]');
    await expect(tree.locator('[data-path="$.method"]')).toContainText('"GET"');
    await expect(headers).toContainText("[1]");
    await expect(tree.locator('[data-path="$.headers[0]"]')).toHaveCount(0);
    await headers.getByRole("button", { name: /headers/ }).click();
    await expect(tree.locator('[data-path="$.headers[0]"]')).toBeVisible();
    await tree.locator('[data-path="$.headers[0]"]').getByRole("button").first().click();
    await expect(tree.locator('[data-path="$.headers[0][1]"]')).toContainText('"application/json"');
    await headers.getByRole("button", { name: /headers/ }).click();
    await expect(tree.locator('[data-path="$.headers[0]"]')).toHaveCount(0);
  });

  test("JSONPath filter shows matches and errors inline", async ({ page }) => {
    const filter = page.getByTestId("response-filter");
    await filter.fill("$.method");
    await expect(page.getByTestId("response-filter-result")).toContainText('"GET"');
    await expect(page.getByTestId("response-filter-error")).toHaveCount(0);
    await filter.fill("$[");
    await expect(page.getByTestId("response-filter-error")).toBeVisible();
    await expect(page.getByTestId("response-filter-result")).toHaveCount(0);
    await filter.fill("");
    await expect(page.getByTestId("response-filter-error")).toHaveCount(0);
    await expect(page.getByTestId("response-lines")).toContainText('"method": "GET"');
  });

  test("raw and hex views", async ({ page }) => {
    await page.getByTestId("response-mode-raw").click();
    await expect(page.getByTestId("response-lines")).toContainText('"method": "GET"');
    await page.getByTestId("response-mode-hex").click();
    await expect(rows(page).first()).toContainText("00000000  7b 0a");
  });
});

test("errors are shown with their message", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Blocked by the browser" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-error")).toContainText("The browser blocked the request: Failed to fetch");
});
