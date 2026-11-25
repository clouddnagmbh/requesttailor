// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C (integration): the test-data journal in the web app with the real core.
// Track a POST to rt-testserver (the id comes from the echo's `path`), pick a DELETE request
// that uses {{journal.id}}, see the plan, send it through the guard and see the entry marked
// reverted. The entry never holds the secret token.

import { expect, test } from "@playwright/test";

import { SERVER, fakePicker, fillFolder } from "./env";

const CANARY = "RT-CANARY-journal-web-4c9e";

function collection(): Record<string, string> {
  const req = (name: string, method: string, url: string) =>
    ["info:", `  name: ${name}`, "  type: http", "  seq: 1", "", "http:", `  method: ${method}`, `  url: "${url}"`, ""].join("\n");
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Journal test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n  - secret: true\n    name: token\n    value: ${CANARY}\n`,
    "create.yml": req("Create thing", "POST", "{{baseUrl}}/echo/things"),
    "remove.yml": req("Remove thing", "DELETE", "{{baseUrl}}{{journal.id}}"),
  };
}

test("track a write, revert it by hand and see it marked", async ({ page }) => {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Journal test");
  await page.getByTestId("env-switcher").selectOption("Local");

  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("test-data journal");
  await palette.getByRole("option").filter({ hasText: "Test-data journal" }).click();
  await page.getByTestId("journal-track").check();
  await page.getByTestId("journal-id-path").fill("$.path");
  await page.getByTestId("journal-id-path").blur();

  await page.getByTestId("tree-request").filter({ hasText: "Create thing" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  await page.keyboard.press("ControlOrMeta+k");
  await palette.getByRole("combobox").fill("test-data journal");
  await palette.getByRole("option").filter({ hasText: "Test-data journal" }).click();
  const entry = page.getByTestId("journal-entry");
  await expect(entry).toHaveCount(1);
  await expect(entry).toContainText("POST");
  await expect(entry).toContainText("/echo/things");
  expect(await page.getByTestId("journal-panel").innerText()).not.toContain(CANARY);

  await page.getByTestId("journal-revert").click();
  await page.getByTestId("journal-pick").selectOption({ label: "Remove thing" });
  await page.getByTestId("journal-plan-button").click();
  await expect(page.getByTestId("journal-plan-url")).toContainText("/echo/things");
  await expect(page.getByTestId("journal-plan")).toHaveAttribute("data-production", "false");
  await page.getByTestId("journal-send").click();
  await expect(page.getByTestId("journal-outcome")).toContainText("Reverted");
  await expect(entry.filter({ hasText: "POST" })).toHaveAttribute("data-reverted", "true");
});
