// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand H: GraphQL and Protobuf changes in the contract view, the changelog and the
// breaking-change notes of a diff of two revisions, against the mock backend's scripted Orders
// API (apps/ui/src/commands/mock-contract.ts). The real core is tested in Rust
// (crates/rt-format/tests/contract_{graphql,protobuf}.rs, crates/rt-cli/tests/
// changelog_schemas.rs) and through rt-wasm in apps/web/src/contract-schemas.test.ts.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

type W = { __rt: { backend: { git: { scenario(name: string): Promise<void> } } } };

async function openContract(page: Page, demo: boolean) {
  await page.goto(demo ? "/?backend=mock&contract=1" : "/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("API contract");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("contract-panel")).toBeVisible();
}

test("the changes of a range list the GraphQL and Protobuf parts after the OpenAPI ones", async ({ page }) => {
  await openContract(page, true);
  await page.getByTestId("contract-from").fill("HEAD");
  await page.getByTestId("contract-changes").click();
  await expect(page.getByTestId("contract-changes-summary")).toContainText("3 breaking");

  const titles = page.getByTestId("contract-schema-title");
  await expect(titles).toHaveText(["GraphQL", "Protobuf"]);
  const graphql = page.locator('[data-testid="contract-schema-change"][data-schema="graphql"]');
  await expect(graphql).toHaveCount(3);
  await expect(graphql.first()).toContainText("Order.total");
  await expect(graphql.first()).toContainText("orders/get-order.yml");
  await expect(page.locator('[data-testid="contract-schema-change"][data-schema="graphql"][data-breaking="true"]')).toHaveCount(2);
  const summary = page.locator('[data-testid="contract-schema-summary"][data-schema="protobuf"]');
  await expect(summary).toContainText("1 breaking");
  await expect(page.locator('[data-testid="contract-schema-change"][data-schema="protobuf"]')).toHaveCount(2);

  // Nothing overflows the panel horizontally.
  const scroll = await page.getByTestId("contract-panel").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(scroll).toBeLessThanOrEqual(0);
  await page.screenshot({ path: test.info().outputPath("contract-schemas.png") });
});

test("the changelog has a section for each schema, breaking changes first", async ({ page }) => {
  await openContract(page, true);
  await page.getByTestId("contract-from").fill("HEAD");
  await page.getByTestId("contract-changelog").click();
  const text = page.getByTestId("contract-changelog-text");
  await expect(text).toContainText("## GraphQL");
  await expect(text).toContainText("## Protobuf");
  await expect(text).toContainText("`Order.total`");
  await expect(text).toContainText("(affects `orders/get-order.yml`)");
  const body = (await text.textContent()) ?? "";
  expect(body.indexOf("## GraphQL")).toBeGreaterThan(body.indexOf("## Breaking changes"));
  expect(body.indexOf("## Protobuf")).toBeGreaterThan(body.indexOf("## GraphQL"));
  expect(body.indexOf("### Breaking changes", body.indexOf("## GraphQL"))).toBeLessThan(
    body.indexOf("### Other changes", body.indexOf("## GraphQL")),
  );
});

test("without a spec or a schema the view and the notes stay quiet", async ({ page }) => {
  await openContract(page, false);
  await expect(page.getByTestId("contract-none")).toBeVisible();
  await expect(page.getByTestId("contract-schema-title")).toHaveCount(0);
});

test("the diff of two revisions names the breaking changes of each schema", async ({ page }) => {
  await page.goto("/?backend=mock&contract=1");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => (window as unknown as W).__rt.backend.git.scenario("showcase"));
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-branch").click();
  await page.getByRole("menuitem", { name: "Compare with" }).click();
  await page.getByRole("menu").last().getByRole("menuitem").first().click();
  const view = page.getByTestId("semantic-diff-view");
  await expect(view).toHaveAttribute("data-scope", "between");
  const notes = page.getByTestId("contract-notes");
  await expect(notes).toBeVisible();
  // three of the OpenAPI spec, two of GraphQL, one of Protobuf
  await expect(notes).toContainText("Breaking changes of the API (6)");
  await expect(page.locator('[data-testid="contract-note"][data-source="OpenAPI"]')).toHaveCount(3);
  await expect(page.locator('[data-testid="contract-note"][data-source="GraphQL"]')).toHaveCount(2);
  await expect(page.locator('[data-testid="contract-note"][data-source="Protobuf"]')).toHaveCount(1);
  await expect(page.locator('[data-testid="contract-note"][data-source="GraphQL"]').first()).toContainText("Order.total");
  await expect(page.locator('[data-testid="contract-note"][data-source="GraphQL"]').first()).toContainText("orders/get-order.yml");
  await page.screenshot({ path: test.info().outputPath("contract-notes.png") });
});

test("a diff without schemas shows only what the spec breaks", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => (window as unknown as W).__rt.backend.git.scenario("showcase"));
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-branch").click();
  await page.getByRole("menuitem", { name: "Compare with" }).click();
  await page.getByRole("menu").last().getByRole("menuitem").first().click();
  await expect(page.getByTestId("semantic-diff-view")).toHaveAttribute("data-scope", "between");
  await expect(page.getByTestId("contract-notes")).toHaveCount(0);
});
