// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand F: the contract view against the mock backend's scripted Orders API
// (apps/ui/src/commands/mock-contract.ts). The real core is tested in Rust
// (crates/rt-format/tests/contract.rs, crates/rt-cli/tests/changelog.rs) and through rt-wasm in
// apps/web/src/contract.test.ts.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function open(page: Page, demo: boolean) {
  await page.goto(demo ? "/?backend=mock&contract=1" : "/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("API contract");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("contract-panel")).toBeVisible();
}

test("without a spec the view says where to put one", async ({ page }) => {
  await open(page, false);
  await expect(page.getByTestId("contract-none")).toContainText("openapi.yaml");
  await expect(page.getByTestId("contract-none")).toContainText("extensions.requesttailor.contract.spec");
  await expect(page.getByTestId("contract-check")).toHaveCount(0);
});

test("checks the collection against the spec and lists problems and uncovered operations", async ({ page }) => {
  await open(page, true);
  await expect(page.getByTestId("contract-spec-title")).toHaveText("Orders API");
  await expect(page.getByTestId("contract-spec-version")).toHaveText("2.0.0");
  await expect(page.getByTestId("contract-spec-path")).toHaveText("openapi.yaml");
  await expect(page.getByTestId("contract-summary")).toContainText("4 of 6 operations");
  await expect(page.getByTestId("contract-summary")).toContainText("4 problems");
  const findings = page.getByTestId("contract-finding");
  await expect(findings).toHaveCount(4);
  await expect(findings.first()).toHaveAttribute("data-kind", "no-operation");
  await expect(page.getByTestId("contract-uncovered-op")).toHaveText(["GET /orders/{id}/items", "DELETE /orders/{id}"]);

  // Nothing overflows the panel horizontally.
  const box = await page.getByTestId("contract-panel").boundingBox();
  const scroll = await page.getByTestId("contract-panel").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(box!.width).toBeGreaterThan(300);
  expect(scroll).toBeLessThanOrEqual(0);
  await page.screenshot({ path: test.info().outputPath("contract-check.png") });

  // A revision that does not exist is reported, the view stays.
  await page.getByTestId("contract-rev").fill("no-such-rev");
  await page.getByTestId("contract-check").click();
  await expect(page.getByTestId("contract-error")).toBeVisible();
});

test("shows the breaking changes between two revisions and the changelog", async ({ page }) => {
  await open(page, true);
  // The mock repository has one commit.
  await page.getByTestId("contract-from").fill("HEAD");
  await page.getByTestId("contract-changes").click();
  await expect(page.getByTestId("contract-changes-summary")).toContainText("1.1.0 → 2.0.0");
  await expect(page.getByTestId("contract-changes-summary")).toContainText("3 breaking");
  const changes = page.getByTestId("contract-change");
  await expect(changes).toHaveCount(5);
  await expect(page.locator('[data-testid="contract-change"][data-breaking="true"]')).toHaveCount(3);
  await expect(changes.first()).toContainText("DELETE /orders/{id}");
  await expect(changes.first()).toContainText("orders/delete-order.yml");

  await page.getByTestId("contract-changelog").click();
  const text = page.getByTestId("contract-changelog-text");
  await expect(text).toContainText("# Orders API 1.1.0 → 2.0.0");
  await expect(text).toContainText("## Breaking changes");
  await expect(text).toContainText("`DELETE /orders/{id}`");
  await page.screenshot({ path: test.info().outputPath("contract-changes.png") });
});

test("a finding opens its request", async ({ page }) => {
  await open(page, true);
  await page.getByTestId("contract-finding").filter({ hasText: "list-orders" }).first().getByRole("button").click();
  await expect(page.getByTestId("git-view")).toHaveCount(0);
});
