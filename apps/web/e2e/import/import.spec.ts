// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand B: the import dialog (mock backend). Drop or pick a file, see the format,
// counts and the loss report grouped by kind, import, and the collection opens.

import { expect, test, type Page } from "@playwright/test";

const POSTMAN = JSON.stringify({
  info: { name: "Shop", schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
  item: [],
});

async function openDialog(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("import").click();
  await expect(page.getByTestId("import-dialog")).toBeVisible();
}

/** Drops `files` on the drop zone like a file manager would. */
async function drop(page: Page, files: { name: string; text: string }[]) {
  const transfer = await page.evaluateHandle((files) => {
    const dt = new DataTransfer();
    for (const f of files) dt.items.add(new File([f.text], f.name, { type: "application/json" }));
    return dt;
  }, files);
  await page.getByTestId("import-drop").dispatchEvent("drop", { dataTransfer: transfer });
}

test("drop a Postman export, read the loss report, import, the collection opens", async ({ page }) => {
  await openDialog(page);
  await drop(page, [
    { name: "shop.postman_collection.json", text: POSTMAN },
    { name: "prod.postman_environment.json", text: "{}" },
  ]);

  await expect(page.getByTestId("import-detected")).toHaveText("Postman");
  await expect(page.getByTestId("import-name")).toHaveText("shop");
  await expect(page.getByTestId("import-counts")).toHaveText("4 requests, 1 folder, 2 environments");
  await expect(page.getByTestId("import-plan")).toContainText("Loss report: 4");
  await expect(page.getByTestId("import-plan")).toContainText("(1 dropped, 3 approximated)");

  // Grouped by kind; each line says dropped or approximated, where, and what.
  const groups = page.getByTestId("import-group");
  await expect(groups).toHaveCount(4);
  await expect(groups.locator("summary")).toHaveText([
    /Scripts\s*1, 1 approximated/,
    /Tests\s*1, 1 approximated/,
    /Auth and credentials\s*1, 1 approximated/,
    /Variables and secrets\s*1, 1 dropped/,
  ]);
  const secret = page.getByTestId("import-loss").filter({ hasText: "secret value of `token`" });
  await expect(secret).toContainText("dropped");
  await expect(secret).toContainText("environment Production");

  await page.getByTestId("import-write").click();
  await expect(page.getByTestId("import-dialog")).toBeHidden();
  await expect(page.getByTestId("collection-name")).toHaveText("shop");
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
});

test("pick a file with the button, by keyboard only", async ({ page }) => {
  await openDialog(page);
  const chooser = page.waitForEvent("filechooser");
  await page.getByTestId("import-file").focus();
  await page.keyboard.press("Space");
  await (await chooser).setFiles({ name: "api.har", mimeType: "application/json", buffer: Buffer.from('{"log": {}}') });
  await expect(page.getByTestId("import-detected")).toHaveText("HAR");
  await expect(page.getByTestId("import-group")).toHaveCount(1);
  // Escape closes and discards the plan.
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("import-dialog")).toBeHidden();
});

test("paste cURL text; a lossless import says so; copy the report", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openDialog(page);
  await expect(page.getByTestId("import-analyze-text")).toBeDisabled();
  await page.getByTestId("import-text").fill("curl https://api.example.com/users");
  await page.getByTestId("import-analyze-text").click();
  await expect(page.getByTestId("import-detected")).toHaveText("cURL");
  await expect(page.getByTestId("import-complete")).toHaveText("Everything was imported exactly.");
  await page.getByTestId("import-copy").click();
  await expect(page.getByTestId("import-copy")).toHaveText("Copied");
  const text = await page.evaluate(() => navigator.clipboard.readText());
  expect(text).toContain('Import of pasted.txt (cURL) as "api.example.com"');
  expect(text).toContain("Nothing was lost");
  // Back returns to the picker.
  await page.getByTestId("import-back").click();
  await expect(page.getByTestId("import-drop")).toBeVisible();
});

test("an invalid file shows the reason and nothing is imported", async ({ page }) => {
  await openDialog(page);
  await drop(page, [{ name: "broken.json", text: "{INVALID" }]);
  await expect(page.getByTestId("import-error")).toHaveText(/broken\.json is not valid JSON/);
  await expect(page.getByTestId("import-plan")).toHaveCount(0);
  await expect(page.getByTestId("import-drop")).toBeVisible();
});

test("an explicit format overrides detection", async ({ page }) => {
  await openDialog(page);
  await page.getByTestId("import-format").selectOption("openapi");
  await drop(page, [{ name: "spec.json", text: "{}" }]);
  await expect(page.getByTestId("import-detected")).toHaveText("OpenAPI");
  await expect(page.getByTestId("import-group").locator("summary")).toHaveText([/Bodies/, /Documentation/]);
});
