// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 19, strand G: the sequence view (palette "Sequence diagram: …", the
// button at a finished run), the own SVG with participants, arrows and notes, the keyboard
// (arrows, Enter opens the request, c copies), values only behind a switch with a warning,
// export as Markdown (copy and "Save as .md" as an unsaved companion file) and no horizontal
// scrolling at 900 px. Mock backend: the core builds the real diagrams (rt-app tests, CLI tests,
// `shells/sequence.e2e.ts`); here the view is under test.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function palette(page: Page, text: string) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(text);
  await dialog.getByRole("option").filter({ hasText: text }).first().click();
}

async function openSession(page: Page) {
  await palette(page, "Sequence diagram: this session");
  await expect(page.getByTestId("sequence-panel")).toBeVisible();
  await expect(page.getByTestId("sequence-svg")).toBeVisible();
}

test("draws participants, arrows and a note from the model", async ({ page }) => {
  await openCollection(page);
  await openSession(page);
  await expect(page.getByTestId("sequence-participant")).toHaveText(["Client", "api.example.com", "auth.example.com"]);
  await expect(page.getByTestId("sequence-message")).toHaveCount(6);
  await expect(page.getByTestId("sequence-message").first()).toContainText("POST /token");
  await expect(page.getByTestId("sequence-note")).toContainText("1 of 2 tests failed");
  await expect(page.getByTestId("sequence-foot")).toContainText("6 messages, 3 participants");
});

test("the query and the values are off until asked for, values with a warning", async ({ page }) => {
  await openCollection(page);
  await openSession(page);
  await expect(page.getByTestId("sequence-values-warning")).toHaveCount(0);
  await expect(page.getByTestId("sequence-svg")).not.toContainText("?limit=5");
  await page.getByTestId("sequence-query").check();
  await expect(page.getByTestId("sequence-svg")).toContainText("GET /orders?limit=5");
  await page.getByTestId("sequence-values").check();
  await expect(page.getByTestId("sequence-values-warning")).toContainText("personal data");
  await expect(page.getByTestId("sequence-svg")).toContainText("id=42");
  await page.getByTestId("sequence-values").uncheck();
  await expect(page.getByTestId("sequence-svg")).not.toContainText("id=42");
  await expect(page.getByTestId("sequence-values-warning")).toHaveCount(0);
});

test("arrow keys move between messages, Enter opens the request, c copies", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openCollection(page);
  await openSession(page);
  const diagram = page.getByTestId("sequence-diagram");
  await diagram.focus();
  await page.keyboard.press("ArrowDown");
  await expect(page.getByTestId("sequence-message").first()).toHaveAttribute("data-selected", "true");
  await expect(page.getByTestId("sequence-live")).toContainText("Message 1 of 6");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(page.getByTestId("sequence-live")).toContainText("Message 3 of 6: request from Client to api.example.com, GET /orders");
  await page.keyboard.press("End");
  await expect(page.getByTestId("sequence-live")).toContainText("Message 6 of 6");
  await page.keyboard.press("Home");
  await page.keyboard.press("c");
  await expect(page.getByTestId("sequence-status")).toContainText("Copied as Markdown");
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied.startsWith("```mermaid\nsequenceDiagram\n")).toBe(true);
  expect(copied.trimEnd().endsWith("```")).toBe(true);
  // Message 3 belongs to List users.
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("tab").filter({ hasText: "List users" }).first()).toBeVisible();
});

test("exports Markdown and Mermaid text, and saves an unsaved companion file", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openCollection(page);
  await openSession(page);
  await page.getByTestId("sequence-copy-mermaid").click();
  await expect(page.getByTestId("sequence-status")).toContainText("Copied the Mermaid text");
  expect((await page.evaluate(() => navigator.clipboard.readText())).startsWith("sequenceDiagram\n")).toBe(true);
  await page.getByTestId("sequence-copy").click();
  expect((await page.evaluate(() => navigator.clipboard.readText())).startsWith("```mermaid\n")).toBe(true);
  await page.getByTestId("sequence-save").click();
  await expect(page.getByTestId("sequence-status")).toContainText(/Added sequence\/session-\d{8}-\d{6}\.md/);
});

test("the button at a finished run opens the latest run's diagram", async ({ page }) => {
  await openCollection(page);
  const row = page.getByTestId("tree-folder").filter({ hasText: "users" });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true");
  await page.getByTestId("runner-sequence").click();
  await expect(page.getByTestId("sequence-panel")).toBeVisible();
  await expect(page.getByTestId("sequence-source")).toHaveValue("run");
  await expect(page.getByTestId("sequence-message").first()).toBeVisible();
});

test("a long diagram draws only the rows in the window and the page never scrolls sideways", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await openCollection(page, "&sequence=big");
  await openSession(page);
  await expect(page.getByTestId("sequence-foot")).toContainText("2000 messages, 8 participants");
  const drawn = await page.getByTestId("sequence-message").count();
  expect(drawn).toBeGreaterThan(5);
  expect(drawn).toBeLessThan(60);
  const diagram = page.getByTestId("sequence-diagram");
  await diagram.evaluate((el) => (el.scrollTop = 20_000));
  await expect(page.getByTestId("sequence-message").first()).toHaveAttribute("data-index", /^(2[6-9]\d|[3-9]\d\d)$/);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth);
  expect(overflow).toBe(false);
});
