// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand A in the browser with the real core (rt-wasm, OPFS folders): a folder with two
// collections opens as two sidebar nodes (discovery skips hidden folders and node_modules), each
// sends with its own environment against rt-testserver, Mod+S writes both, search covers both,
// "Add collection…" adds another folder, and a reload brings the whole sidebar back.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile } from "./env";

const MOD = "ControlOrMeta";

const manifest = (name: string) => `opencollection: "1.0.0"\ninfo:\n  name: ${name}\n`;
const env = (name: string, path: string) =>
  `name: ${name}\nvariables:\n  - name: baseUrl\n    value: ${SERVER}${path}\n`;
const request = (name: string, url: string) =>
  ["info:", `  name: ${name}`, "  type: http", "  seq: 1", "", "http:", "  method: GET", `  url: "${url}"`, ""].join("\n");

function apis(): Record<string, string> {
  return {
    "billing/opencollection.yml": manifest("Billing"),
    "billing/environments/Billing local.yml": env("Billing local", "/echo/billing"),
    "billing/ping.yml": request("Ping billing", "{{baseUrl}}/ping"),
    "orders/opencollection.yml": manifest("Orders"),
    "orders/environments/Orders local.yml": env("Orders local", "/echo/orders"),
    "orders/ping.yml": request("Ping orders", "{{baseUrl}}/ping"),
    ".hidden/opencollection.yml": manifest("Hidden"),
    "node_modules/x/opencollection.yml": manifest("Decoy"),
    "README.md": "two collections\n",
  };
}

const node = (page: Page, name: string) => page.getByTestId("tree-collection").filter({ hasText: name });
const treeRequest = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name });

async function openApis(page: Page) {
  await fakePicker(page, "apis");
  await page.goto("/");
  await fillFolder(page, apis(), "apis");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Billing/, /Orders/]);
}

test("two collections from one folder: own environments, save all, search over both", async ({ page }) => {
  await openApis(page);
  await expect(page.getByTestId("collection-name")).toHaveText("Billing");

  await treeRequest(page, "Ping billing").click();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("env-active")).toHaveText("Billing local");

  await node(page, "Orders").click();
  await treeRequest(page, "Ping orders").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Orders");
  await expect(page.getByTestId("env-active")).toHaveText("Orders local");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("request-editor")).toContainText("/echo/orders/ping");

  // A new request in each collection, both written by one Mod+S.
  for (const [collection, name] of [
    ["Billing", "Refund"],
    ["Orders", "Cancel"],
  ]) {
    await node(page, collection).hover();
    await page.getByRole("button", { name: `New request in ${collection}` }).click();
    await page.getByTestId("new-request-name").fill(name);
    await page.getByTestId("new-request-name").press("Enter");
  }
  await expect(page.getByTestId("collection-dirty")).toHaveCount(2);
  await page.keyboard.press(`${MOD}+s`);
  await expect(page.getByTestId("collection-dirty")).toHaveCount(0);
  expect(await readFolderFile(page, "billing/refund.yml", "apis")).toContain("name: Refund");
  expect(await readFolderFile(page, "orders/cancel.yml", "apis")).toContain("name: Cancel");

  await page.getByTestId("view-search").click();
  await page.getByTestId("search-input").fill("ping");
  await expect(page.getByTestId("search-group")).toHaveCount(2);
});

test("Add collection… adds another folder; a reload brings the sidebar back", async ({ page }) => {
  await openApis(page);
  await fillFolder(page, { "opencollection.yml": manifest("Payments"), "ping.yml": request("Ping payments", "{{baseUrl}}/p") }, "payments");
  await page.evaluate(() => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => (await navigator.storage.getDirectory()).getDirectoryHandle("payments"),
    });
  });
  await page.getByTestId("add-collection").click();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Billing/, /Orders/, /Payments/]);
  await expect(page.getByTestId("collection-name")).toHaveText("Payments");

  await node(page, "Orders").click();
  await treeRequest(page, "Ping orders").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Orders");

  await page.reload();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Billing/, /Orders/, /Payments/]);
  await expect(page.getByTestId("collection-name")).toHaveText("Orders");

  // Removing the added folder leaves the first one.
  await node(page, "Payments").click({ button: "right" });
  await page.getByRole("menuitem", { name: "Remove from sidebar" }).click();
  await expect(page.getByTestId("tree-collection")).toHaveText([/Billing/, /Orders/]);
});
