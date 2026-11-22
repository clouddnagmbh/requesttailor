// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand E: the Search view (mock backend). Results are grouped by request with a
// method badge and highlighted snippets; arrows and Enter open the request, Escape clears;
// unsaved edits are found.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const tab = (page: Page, name: string) => page.getByTestId("tab").filter({ hasText: name });
const hits = (page: Page) => page.getByTestId("search-hit");
const groups = (page: Page) => page.getByTestId("search-group");

test("results are grouped by request with method badge and highlighted matches", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("view-search").click();
  const input = page.getByTestId("search-input");
  await expect(input).toBeVisible();
  await input.fill("USER");

  await expect(groups(page)).toHaveCount(2);
  await expect(groups(page).nth(0)).toHaveAttribute("data-path", "users/list-users.yml");
  await expect(groups(page).nth(0)).toContainText("GET");
  await expect(groups(page).nth(0)).toContainText("List users");
  await expect(groups(page).nth(1)).toContainText("POST");
  await expect(groups(page).nth(1).getByTestId("search-hit")).toHaveCount(2);
  // The best hit (name) comes first in each group, then the URL.
  await expect(groups(page).nth(0).getByTestId("search-hit")).toHaveText([/Name\s*List users/, /URL\s*\{\{baseUrl\}\}\/users/]);
  // Case-insensitive, and only the match is marked.
  await expect(groups(page).nth(0).locator("mark")).toHaveText(["user", "user"]);
  await expect(page.getByTestId("search-summary")).toHaveText("4 matches in 2 requests");
});

test("searches headers, bodies and auth, and finds unsaved edits", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("view-search").click();
  const input = page.getByTestId("search-input");

  await input.fill("application/json");
  await expect(hits(page)).toHaveCount(1);
  await expect(hits(page).first()).toHaveAttribute("data-field", "header");
  await expect(hits(page).first()).toContainText("Accept: application/json");

  await input.fill("ada");
  await expect(hits(page).first()).toHaveAttribute("data-field", "body");
  await expect(hits(page).first().locator("mark")).toHaveText("Ada");

  await input.fill("{{token}}");
  await expect(hits(page).first()).toHaveAttribute("data-field", "auth");

  // Edit a request (not saved), search for the new text.
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, name: "Gesundheit 🚀 Grüße" });
  });
  await input.fill("grüße");
  await expect(hits(page)).toHaveCount(1);
  await expect(hits(page).first().locator("mark")).toHaveText("Grüße");

  await input.fill("nothing-like-this");
  await expect(hits(page)).toHaveCount(0);
  await expect(page.getByTestId("search-summary")).toHaveText("No matches.");
});

test("keyboard: arrows move, Enter opens the request, Escape clears", async ({ page }) => {
  await openCollection(page);
  // The palette command opens the view with the field focused.
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Search in collection");
  await page.keyboard.press("Enter");
  const input = page.getByTestId("search-input");
  await expect(input).toBeFocused();

  await page.keyboard.type("user");
  await expect(hits(page)).toHaveCount(4);
  await expect(hits(page).nth(0)).toHaveAttribute("aria-selected", "true");
  await expect(input).toHaveAttribute("aria-activedescendant", "rt-search-hit-0");
  // Display order: List users (name, URL), then Create user (name, URL).
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(hits(page).nth(2)).toHaveAttribute("aria-selected", "true");
  await expect(hits(page).nth(2)).toHaveAttribute("data-field", "name");
  await expect(input).toHaveAttribute("aria-activedescendant", "rt-search-hit-2");
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("ArrowDown");
  await expect(hits(page).nth(3)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("Enter");
  await expect(tab(page, "List users")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("tab")).toHaveCount(1);
  await expect(input).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(tab(page, "Create user")).toHaveAttribute("aria-selected", "true");

  await page.keyboard.press("Escape");
  await expect(input).toHaveValue("");
  await expect(hits(page)).toHaveCount(0);
});

test("a click on a hit opens its request", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("view-search").click();
  await page.getByTestId("search-input").fill("health");
  await hits(page).first().click();
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
});
