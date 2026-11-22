// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 6, strand F: the first-start question and collection tabs against the mock backend.
// `?welcome=1` gives the mock a bundled plugin (a stand-in for OData, installed disabled,
// apps/ui/src/commands/mock-plugins.ts). Gate: whoever does not pick it sees no plugin tab
// and no SAP text; picking it shows its tabs. Its collection tab proposes additions, which
// are confirmed first and then become unsaved changes.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function welcome(page: Page) {
  await page.goto("/?backend=mock&welcome=1");
  const dialog = page.getByTestId("welcome-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByRole("heading", { name: "What do you work with?" })).toBeVisible();
  await expect(dialog.locator("label")).toHaveText([/REST, GraphQL & co\. \(no extras\)/, /SAP and OData/]);
  await expect(page.getByTestId("welcome-none")).toBeChecked();
  return dialog;
}

test("no extras: no plugin tabs and no SAP anywhere", async ({ page }) => {
  const dialog = await welcome(page);
  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);

  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await expect(page.getByTestId("send")).toBeVisible();
  await expect(page.getByTestId("tab-plugin-odata/query")).toHaveCount(0);
  await expect(page.getByTestId("collection-tabs")).toHaveCount(0);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Collection");
  await expect(page.getByText(/Collection: /)).toHaveCount(0);
  await page.keyboard.press("Escape");

  // The disabled plugin is listed under "Manage plugins", without its description.
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("plugin-item")).toHaveCount(1);
  await expect(page.getByTestId("plugin-enabled")).not.toBeChecked();
  await expect(page.locator("body")).not.toContainText("SAP");
  await page.getByTestId("plugins-close").click();
  await expect(page.locator("body")).not.toContainText("SAP");
  // Answered: not asked again in this session.
  await expect(page.getByTestId("welcome-dialog")).toHaveCount(0);
});

test("choosing the bundled plugin shows its tabs; additions are confirmed first", async ({ page }) => {
  await welcome(page);
  await page.getByTestId("welcome-choice").check();
  await expect(page.getByTestId("welcome-none")).not.toBeChecked();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("welcome-dialog")).toHaveCount(0);

  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await expect(page.getByTestId("tab-plugin-odata/query")).toBeVisible();

  // The collection tab: from the header button, rendered with the selected environment.
  await page.getByTestId("collection-tabs").click();
  const panel = page.getByTestId("collection-panel");
  await expect(panel.getByTestId("collection-tab")).toHaveText(["OData"]);
  const tab = panel.getByTestId("plugin-tab");
  await expect(tab.getByRole("heading", { name: "OData: entities" })).toBeVisible();
  await expect(tab).toContainText("Request .");

  // Proposed additions: shown, nothing changes on Cancel.
  await tab.getByRole("button", { name: "Add to collection" }).click();
  const additions = page.getByTestId("additions-dialog");
  await expect(additions.getByTestId("additions-requests").locator("li")).toHaveText([
    /GET\s+Sales Orders \/ List sales orders\s*$/,
    /GET\s+users \/ List users 2\s+\(name taken, renamed\)/,
  ]);
  await expect(additions.getByTestId("additions-environments").locator("li")).toHaveText([
    /S4 DEV\s+new:\s+baseUrl, password \(secret, set it yourself\)/,
    /Local\s+adds to the existing one:\s+sapClient/,
  ]);
  await page.getByTestId("additions-cancel").click();
  await expect(additions).toHaveCount(0);
  await expect(page.getByTestId("save")).toBeDisabled();

  // Confirmed: unsaved requests and environments.
  await tab.getByRole("button", { name: "Add to collection" }).click();
  await page.getByTestId("additions-confirm").click();
  await expect(additions).toHaveCount(0);
  await expect(page.getByTestId("save")).toBeEnabled();
  await expect(page.getByTestId("tree-request").filter({ hasText: "List users 2" })).toBeVisible();
  await expect(page.getByTestId("tree-request").filter({ hasText: "List sales orders" })).toBeVisible();
  const envs = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: { environments(): Promise<{ name: string; variables: { name: string; value: string | null }[] }[]> } } }).__rt.backend;
    return (await b.environments()).map((e) => [e.name, e.variables.map((v) => `${v.name}=${v.value}`).join(",")]);
  });
  expect(envs).toContainEqual(["S4 DEV", "baseUrl=https://s4-dev.example.com,password=null"]);
  expect(envs.find(([n]) => n === "Local")?.[1]).toMatch(/^baseUrl=http:\/\/127\.0\.0\.1:47811,.*sapClient=100$/);

  // Opening a request shows the editor again; the palette reopens the collection tab.
  await page.getByTestId("tree-request").filter({ hasText: "List sales orders" }).click();
  await expect(panel).toHaveCount(0);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Collection: OData");
  await page.keyboard.press("Enter");
  await expect(panel).toBeVisible();
});
