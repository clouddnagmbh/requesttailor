// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The OpenAPI wizard against the mock backend (`apps/ui/src/commands/mock-openapi.ts`):
// import a spec with chosen tags and environments, Swagger 2, and the run wizard
// ("Ready to send?") with the spec check of the response.

import { expect, test, type Page } from "@playwright/test";

import { PETSTORE3, SWAGGER2 } from "./fixtures";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function openWizard(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).not.toHaveText("");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Import OpenAPI");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("openapi-wizard")).toBeVisible();
}

async function dropSpec(page: Page, name: string, spec: unknown) {
  const transfer = await page.evaluateHandle(
    ({ name, text }) => {
      const dt = new DataTransfer();
      dt.items.add(new File([text], name, { type: "application/json" }));
      return dt;
    },
    { name, text: JSON.stringify(spec, null, 2) },
  );
  await page.getByTestId("openapi-drop").dispatchEvent("drop", { dataTransfer: transfer });
}

/** Imports the petstore into the open collection, all operations. */
async function importPetstore(page: Page) {
  await openWizard(page);
  await dropSpec(page, "petstore.json", PETSTORE3);
  await page.getByTestId("openapi-next").click();
  await page.getByTestId("openapi-next").click();
  await page.getByTestId("openapi-target-folder").click();
  await page.getByTestId("openapi-import").click();
  await expect(page.getByTestId("openapi-result")).toBeVisible();
  await page.getByTestId("openapi-open-first").click();
  await expect(page.getByTestId("openapi-dialog")).toBeHidden();
  await page.getByTestId("env-switcher").selectOption("Production");
}

async function sendPet(page: Page) {
  await page.getByTestId("tree-request").filter({ hasText: "Info for a specific pet" }).click();
  await page.getByTestId("send").click();
}

test("import Petstore 3 from a file, choose two tags, check the environments", async ({ page }) => {
  await openWizard(page);
  await dropSpec(page, "petstore.json", PETSTORE3);

  const overview = page.getByTestId("openapi-overview");
  await expect(overview).toContainText("Petstore");
  await expect(page.getByTestId("stat-operations")).toContainText("4");
  await expect(page.getByTestId("stat-servers")).toContainText("2");
  await page.getByTestId("openapi-next").click();

  // Two of three tags.
  await page.getByTestId("openapi-none").click();
  await expect(page.getByTestId("openapi-selected")).toContainText("0");
  await page.getByTestId("openapi-group").filter({ hasText: "pets" }).getByRole("checkbox").first().check();
  await page.getByTestId("openapi-group").filter({ hasText: "store" }).getByRole("checkbox").first().check();
  await expect(page.getByTestId("openapi-selected")).toContainText("3");

  // Servers become environments with editable names.
  const names = page.getByTestId("openapi-env-name");
  await expect(names).toHaveCount(2);
  await expect(names.nth(0)).toHaveValue("Production");
  await expect(names.nth(1)).toHaveValue("Staging");
  await names.nth(1).fill("QA");
  await page.getByTestId("openapi-next").click();

  await page.getByTestId("openapi-target-folder").click();
  await expect(page.getByTestId("openapi-keep-spec")).toBeChecked();
  await page.getByTestId("openapi-import").click();

  await expect(page.getByTestId("openapi-result")).toBeVisible();
  await expect(page.getByTestId("openapi-result-envs")).toContainText("Production");
  await expect(page.getByTestId("openapi-result-envs")).toContainText("QA");
  await expect(page.getByTestId("openapi-losses")).toBeVisible();
  await page.getByTestId("openapi-open-first").click();

  await expect(page.getByTestId("tree-request").filter({ hasText: "List all pets" })).toBeVisible();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Place an order" })).toBeVisible();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Get a user" })).toHaveCount(0);
  const envs = await page.getByTestId("env-switcher").locator("option").allTextContents();
  expect(envs).toEqual(expect.arrayContaining(["Production", "QA"]));
});

test("import a Swagger 2.0 spec", async ({ page }) => {
  await openWizard(page);
  await dropSpec(page, "legacy.json", SWAGGER2);
  await expect(page.getByTestId("openapi-overview")).toContainText("Legacy Shop");
  await expect(page.getByTestId("openapi-overview")).toContainText("2.0");
  await expect(page.getByTestId("stat-operations")).toContainText("3");
  await page.getByTestId("openapi-next").click();
  await expect(page.getByTestId("openapi-server")).toContainText("https://shop.example.com/api");
  await page.getByTestId("openapi-next").click();
  await page.getByTestId("openapi-target-folder").click();
  await page.getByTestId("openapi-import").click();
  await expect(page.getByTestId("openapi-result")).toBeVisible();
  await page.getByTestId("openapi-open-first").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "Replace an item" })).toBeVisible();
});

test("an OpenAPI file dropped in the import dialog goes to the wizard", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("import").click();
  const transfer = await page.evaluateHandle((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "petstore.json", { type: "application/json" }));
    return dt;
  }, JSON.stringify(PETSTORE3));
  await page.getByTestId("import-drop").dispatchEvent("drop", { dataTransfer: transfer });
  await expect(page.getByTestId("import-dialog")).toBeHidden();
  await expect(page.getByTestId("openapi-overview")).toContainText("Petstore");
});

test("the New dialog's 'From OpenAPI' tile opens the wizard", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).not.toHaveText("");
  await page.keyboard.press(`${MOD}+n`);
  await page.getByTestId("new-filter").fill("openapi");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("openapi-wizard")).toBeVisible();
  await expect(page.getByTestId("openapi-drop")).toBeVisible();
});

test("sending GET /pets/{petId} with an empty id asks first, then fills and sends", async ({ page }) => {
  await importPetstore(page);
  await sendPet(page);

  const panel = page.getByTestId("ready-to-send");
  await expect(panel).toBeVisible();
  await expect(page.getByTestId("response-status")).toHaveCount(0);
  const field = page.locator('[data-testid="ready-field"][data-field="path:petId"]');
  await expect(field).toContainText("petId");
  await expect(field).toContainText("The id of the pet to retrieve");

  // Invalid first: a clear message, nothing is sent.
  await field.getByRole("textbox").fill("0");
  await page.getByTestId("ready-fill").click();
  await expect(field).toContainText("must be at least 1");
  await expect(panel).toBeVisible();

  await field.getByRole("textbox").fill("42");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(panel).toBeHidden();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  // The value is on the request (unsaved, like any edit): sending again does not ask.
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(panel).toBeHidden();
});

test("the response panel shows how the response differs from the spec", async ({ page }) => {
  await importPetstore(page);
  await sendPet(page);
  await page.locator('[data-testid="ready-field"][data-field="path:petId"]').getByRole("textbox").fill("7");
  await page.getByTestId("ready-fill").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  // The mock echoes the request, not a Pet: `id` and `name` are missing.
  const badge = page.getByTestId("spec-badge");
  await expect(badge).toHaveText(/2 differences/);
  await badge.click();
  const rows = page.getByTestId("spec-difference");
  await expect(rows).toHaveCount(2);
  await expect(rows.first()).toContainText("Body");
  await expect(page.getByTestId("spec-differences")).toContainText("GET /pets/{petId}");

  // An operation without a response schema matches.
  await page.keyboard.press("Escape");
  await page.getByTestId("tree-request").filter({ hasText: "List all pets" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("spec-badge")).toHaveText(/Matches spec/);
});

test("'Send anyway' sends as is, 'Don't ask for this request' stops asking", async ({ page }) => {
  await importPetstore(page);
  await sendPet(page);
  await expect(page.getByTestId("ready-to-send")).toBeVisible();
  await page.getByTestId("ready-send-anyway").click();
  await expect(page.getByTestId("ready-to-send")).toBeHidden();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  // Asks again next time, until told not to.
  await page.getByTestId("send").click();
  await expect(page.getByTestId("ready-to-send")).toBeVisible();
  await page.getByTestId("ready-dont-ask").check();
  await page.getByTestId("ready-send-anyway").click();
  await expect(page.getByTestId("ready-to-send")).toBeHidden();

  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.waitForTimeout(300);
  await expect(page.getByTestId("ready-to-send")).toHaveCount(0);

  // Cancel (Escape) sends nothing.
  await page.getByTestId("tree-request").filter({ hasText: "Place an order" }).click();
  const before = await page.getByTestId("response-status").count();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("ready-to-send")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("ready-to-send")).toBeHidden();
  expect(await page.getByTestId("response-status").count()).toBe(before);
});
