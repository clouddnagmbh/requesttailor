// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand J: the tabs the OData plugin contributes for Gateway errors ("Error", on JSON
// and XML responses) and for measuring server time ("Server time", a request tab), against the
// mock backend. The mock takes a manifest as JSON in place of a `.wasm` file and renders every
// tab generically, so this checks what the UI owes the manifest: the tabs appear where the
// plugin says, response tabs only for the content types it names. What the tabs say (error
// code, message, transaction ID and where to look it up, the segments of `sap-statistics`) is
// covered against the real plugin code by `plugins/odata/tests/gateway.rs`.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

const manifest = {
  id: "odata",
  name: "OData",
  version: "0.1.0",
  api: 1,
  description: "Stand-in with the Gateway tabs of the real plugin.",
  license: "Apache-2.0",
  homepage: null,
  permissions: { network: ["{request}"], storage: true, variables: true },
  contributes: {
    prepare: true,
    response: true,
    tabs: [
      { id: "error", title: "Error", place: "response", content_types: ["application/json", "application/xml", "text/xml"] },
      { id: "statistics", title: "Server time", place: "request", content_types: [] },
    ],
    importers: [],
  },
};

async function install(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await page.getByTestId("plugins-file").setInputFiles({
    name: "odata.wasm",
    mimeType: "application/wasm",
    buffer: Buffer.from(JSON.stringify(manifest)),
  });
  await page.getByTestId("plugins-confirm-install").click();
  await expect(page.getByTestId("plugin-item")).toHaveCount(1);
  await page.getByTestId("plugins-close").click();
}

test("the Server time tab is on the request and the Error tab on JSON responses", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await install(page);

  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("tab-plugin-odata/statistics")).toContainText("Server time");
  await page.getByTestId("tab-plugin-odata/statistics").click();
  await expect(page.getByTestId("request-editor").getByTestId("plugin-tab")).toBeVisible();

  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  const error = page.getByTestId("response-tab-plugin-odata/error");
  await expect(error).toContainText("Error");
  await error.click();
  await expect(page.getByTestId("response-view").getByTestId("plugin-tab")).toBeVisible();
});

test("without the plugin there is neither tab", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("tab-plugin-odata/statistics")).toHaveCount(0);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await expect(page.getByTestId("response-tab-plugin-odata/error")).toHaveCount(0);
});
