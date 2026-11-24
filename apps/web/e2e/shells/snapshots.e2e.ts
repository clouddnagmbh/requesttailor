// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand B (integration): snapshot tests in the web app with the real core. Freeze a
// response of rt-testserver on an OPFS collection, send again and see the assertion pass,
// change what the server answers and see it fail with the difference. Saving writes the
// companion file and the assertion, and no secret.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile } from "./env";

const CANARY = "RT-CANARY-snap-7f3a";

function collection(): Record<string, string> {
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Snapshot test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n  - secret: true\n    name: token\n`,
    "echo.yml": ["info:", "  name: Echo", "  type: http", "  seq: 1", "", "http:", "  method: GET", '  url: "{{baseUrl}}/echo/snap?v=1"', ""].join("\n"),
  };
}

async function send(page: Page) {
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

test("freeze, match, then fail with the difference when the server answer changes", async ({ page }) => {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Snapshot test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "Echo" }).click();

  await send(page);
  await page.getByTestId("snapshot-freeze").click();
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");

  // Re-send: the assertion passes.
  await send(page);
  await page.getByTestId("response-tab-tests").click();
  await expect(page.getByTestId("tests-summary")).toContainText("passed");
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");

  // Save: the request keeps its assertion, the companion file exists, no secret in either.
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await expect.poll(() => readFolderFile(page, "echo.snap.json")).not.toBeNull();
  const snap = (await readFolderFile(page, "echo.snap.json"))!;
  const file = (await readFolderFile(page, "echo.yml"))!;
  expect(file).toContain("matchesSnapshot");
  expect(snap + file).not.toContain(CANARY);
  expect(JSON.parse(snap).version).toBe(1);

  // The server answers differently (the query is part of the echo): the assertion fails.
  await page.getByTestId("url-input").locator(".cm-content").click();
  await page.keyboard.press("End");
  await page.keyboard.press("Backspace");
  await page.keyboard.type("2");
  await send(page);
  await page.getByTestId("response-tab-tests").click();
  await expect(page.getByTestId("tests-summary")).toContainText("failed");
  const badge = page.getByTestId("snapshot-badge");
  await expect(badge).toHaveAttribute("data-state", "differs");
  await badge.click();
  const dialog = page.getByTestId("snapshot-dialog");
  await expect(dialog.getByTestId("snapshot-difference").first()).toContainText("query");
});
