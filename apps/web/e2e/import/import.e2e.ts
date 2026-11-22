// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Gate "migrate a real Postman collection" in the browser with the real core: pick the
// collection with its environment and globals, read the report, import into a new folder
// (OPFS behind a fake picker), the collection opens with its environments, and no secret
// value reaches a file.

import { fileURLToPath } from "node:url";

import { expect, test } from "@playwright/test";

import { fakePicker, readFolderFile } from "../shells/env";

const bundle = fileURLToPath(
  new URL("../../../../testdata/import/postman/synthetic-v21-features.bundle/", import.meta.url),
);
const files = [
  `${bundle}features.postman_collection.json`,
  `${bundle}staging.postman_environment.json`,
  `${bundle}workspace.postman_globals.json`,
];

test("Postman collection with environment and globals: report, import, open", async ({ page }) => {
  await fakePicker(page, "parent");
  await page.goto("/");
  await page.getByTestId("import").click();
  await page.getByTestId("import-file").setInputFiles(files);

  await expect(page.getByTestId("import-detected")).toHaveText("Postman");
  await expect(page.getByTestId("import-name")).toHaveText("Synthetic Features");
  await expect(page.getByTestId("import-counts")).toHaveText("16 requests, 3 folders, 1 environment");
  const secret = page.getByTestId("import-loss").filter({ hasText: "not imported; set it in Environments" });
  await expect(secret.first()).toContainText("dropped");

  await page.getByTestId("import-write").click();
  await expect(page.getByTestId("import-dialog")).toBeHidden();
  await expect(page.getByTestId("collection-name")).toHaveText("Synthetic Features");
  await expect(page.getByTestId("env-switcher")).toContainText("Staging");
  await expect(page.getByTestId("tree-request").first()).toBeVisible();

  const root = await readFolderFile(page, "Synthetic Features/opencollection.yml", "parent");
  expect(root).toContain("name: Synthetic Features");
  const env = await readFolderFile(page, "Synthetic Features/environments/Staging.yml", "parent");
  expect(env).toContain("secret: true");
  expect(env).not.toContain("dummy-key-value");

  // Importing again into the same place is refused: the folder is not empty any more.
  await page.getByTestId("import").click();
  await page.getByTestId("import-file").setInputFiles(files);
  await page.getByTestId("import-write").click();
  await expect(page.getByTestId("import-error")).toHaveText(/already exists and is not empty/);
});
