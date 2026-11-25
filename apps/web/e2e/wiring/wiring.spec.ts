// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Integration wiring of waves 18 and 19 against the mock backend: "Lint collection" and
// "Compare environments" in the sidebar menus, "Compare…" in the environment manager, and the
// "Shrink" action on a failed response.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

test("the collection menu opens the lint and the environment comparison", async ({ page }) => {
  await openDemo(page);
  const node = page.getByTestId("tree-collection").filter({ hasText: "Demo API" });
  await node.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Lint collection" }).click();
  await expect(page.getByTestId("lint")).toBeVisible();

  await node.click({ button: "right" });
  await page.getByRole("menuitem", { name: "Compare environments" }).click();
  await expect(page.getByTestId("lint")).toHaveCount(0);
  await expect(page.getByRole("heading", { name: /Compare environments/ }).first()).toBeVisible();
});

test("the environment manager has a Compare button", async ({ page }) => {
  await openDemo(page);
  await page.getByRole("button", { name: "Environments" }).click();
  const manager = page.getByTestId("env-manager");
  await expect(manager).toBeVisible();
  await manager.getByTestId("env-compare").click();
  await expect(manager).toBeHidden();
  await expect(page.getByRole("heading", { name: /Compare environments/ }).first()).toBeVisible();
});

test("a failed response offers Shrink", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: any } }).__rt.backend;
    const spec = await b.getRequest("users/list-users.yml");
    spec.url = "{{baseUrl}}/users?boom=1";
    await b.updateRequest("users/list-users.yml", spec);
  });
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("500");
  await page.getByTestId("shrink-action").click();
  await expect(page.getByTestId("shrink-panel")).toBeVisible();
});
