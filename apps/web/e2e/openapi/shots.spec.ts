// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Screenshots of the OpenAPI wizard for the visual check, light and dark at 900 and 1440 px:
// `RT_OPENAPI_SHOTS=<dir> npx playwright test e2e/openapi/shots.spec.ts`. Skipped otherwise.

import { expect, test, type Page } from "@playwright/test";

import { PETSTORE3 } from "./fixtures";

const shots = process.env.RT_OPENAPI_SHOTS;
const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function drop(page: Page) {
  const transfer = await page.evaluateHandle((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "petstore.json", { type: "application/json" }));
    return dt;
  }, JSON.stringify(PETSTORE3));
  await page.getByTestId("openapi-drop").dispatchEvent("drop", { dataTransfer: transfer });
}

for (const scheme of ["light", "dark"] as const) {
  for (const width of [900, 1440]) {
    test(`OpenAPI wizard screens (${scheme}, ${width})`, async ({ page }) => {
      test.skip(!shots, "set RT_OPENAPI_SHOTS to write screenshots");
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize({ width, height: width === 900 ? 760 : 900 });
      const shot = (name: string) => page.screenshot({ path: `${shots}/${name}-${scheme}-${width}.png` });
      await page.goto("/?backend=mock");
      await page.getByTestId("open-collection").click();
      await page.keyboard.press(`${MOD}+k`);
      await page.getByTestId("palette-input").fill("Import OpenAPI");
      await page.keyboard.press("Enter");
      await expect(page.getByTestId("openapi-drop")).toBeVisible();
      await page.waitForTimeout(400);
      await shot("1-source");
      await drop(page);
      await expect(page.getByTestId("openapi-overview")).toBeVisible();
      await page.waitForTimeout(300);
      await shot("2-overview");
      await page.getByTestId("openapi-next").click();
      await page.getByTestId("openapi-group").filter({ hasText: "users" }).getByRole("checkbox").first().uncheck();
      await page.waitForTimeout(200);
      await shot("3-choose");
      await page.getByTestId("openapi-next").click();
      await page.getByTestId("openapi-target-folder").click();
      await page.waitForTimeout(200);
      await shot("4-target");
      await page.getByTestId("openapi-import").click();
      await expect(page.getByTestId("openapi-result")).toBeVisible();
      await page.waitForTimeout(300);
      await shot("5-result");
      await page.getByTestId("openapi-open-first").click();
      await page.getByTestId("env-switcher").selectOption("Production");
      await page.getByTestId("tree-request").filter({ hasText: "Place an order" }).click();
      await page.getByTestId("send").click();
      await expect(page.getByTestId("ready-to-send")).toBeVisible();
      await page.waitForTimeout(300);
      await shot("6-ready-body");
      await page.keyboard.press("Escape");
      await page.getByTestId("tree-request").filter({ hasText: "Info for a specific pet" }).click();
      await page.getByTestId("send").click();
      await expect(page.getByTestId("ready-to-send")).toBeVisible();
      await page.locator('[data-testid="ready-field"][data-field="path:petId"]').getByRole("textbox").fill("0");
      await page.getByTestId("ready-fill").click();
      await page.waitForTimeout(300);
      await shot("7-ready-invalid");
      await page.locator('[data-testid="ready-field"][data-field="path:petId"]').getByRole("textbox").fill("42");
      await page.getByTestId("ready-fill").click();
      await expect(page.getByTestId("spec-badge")).toBeVisible();
      await shot("8-badge");
      await page.getByTestId("spec-badge").click();
      await page.waitForTimeout(300);
      await shot("9-differences");
    });
  }
}
