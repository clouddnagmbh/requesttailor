// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand K: the `btp-destinations` importer in the import dialog, against the mock
// backend. The mock takes a manifest as JSON in place of a `.wasm` file and does not run the
// importer itself, so this checks what the UI owes the importer: it is offered with its label
// and extensions, and a destination export file is handed to it. The conversion (variables,
// secrets, loss report, import without requests) is covered by
// `plugins/odata/tests/destinations.rs` against the real plugin code.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

const manifest = {
  id: "odata",
  name: "OData",
  version: "0.1.0",
  api: 1,
  description: "Stand-in with the importer of the real plugin.",
  license: "Apache-2.0",
  homepage: null,
  permissions: { network: [], storage: false, variables: false },
  contributes: {
    prepare: false,
    response: false,
    tabs: [],
    importers: [
      { id: "btp-destinations", label: "SAP BTP destinations (exported JSON or .properties)", extensions: ["json", "properties"] },
    ],
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
  await page.getByTestId("plugins-close").click();
}

const EXPORT = JSON.stringify({
  Name: "RT_S4_DEV",
  Type: "HTTP",
  URL: "https://s4-dev.example.invalid/sap/opu/odata/sap/ZRT_SALES_SRV",
  Authentication: "BasicAuthentication",
  User: "RT_USER",
  "sap-client": "100",
});

test("a new collection from an exported destination", async ({ page }) => {
  await page.goto("/?backend=mock");
  await install(page);
  await page.getByTestId("import").click();
  await page
    .getByTestId("import-format")
    .selectOption({ label: "SAP BTP destinations (exported JSON or .properties) (plugin OData)" });
  await page.getByTestId("import-file").setInputFiles({ name: "RT_S4_DEV.json", mimeType: "application/json", buffer: Buffer.from(EXPORT) });
  await expect(page.getByTestId("import-detected")).toHaveText("Plugin");
  await expect(page.getByTestId("import-plan")).toBeVisible();
});

test("an exported destination into the open collection", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await install(page);
  await page.getByTestId("import").click();
  await page
    .getByTestId("import-format")
    .selectOption({ label: "SAP BTP destinations (exported JSON or .properties) (plugin OData)" });
  await page.getByTestId("import-file").setInputFiles({ name: "s4.properties", mimeType: "text/plain", buffer: Buffer.from("Name=X\nURL=https\\://x.example.invalid\n") });
  await expect(page.getByTestId("import-detected")).toHaveText("Plugin");
  // With a collection open the dialog asks where the import goes.
  await expect(page.getByTestId("import-dialog")).toContainText("Into");
});
