// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: sequence diagrams in the web app with the real core. Two sends on an OPFS
// collection against rt-testserver, then "Sequence diagram: this session": the diagram shows the
// client, the test server and both requests (path without query), the Mermaid text comes from
// the core, and neither the secret nor a query value is in it.

import { expect, test } from "@playwright/test";

import { SERVER, fakePicker, fillFolder } from "./env";

const CANARY = "RT-CANARY-seq-e2e-3c91";

function collection(): Record<string, string> {
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Sequence test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n  - secret: true\n    name: token\n`,
    "echo.yml": ["info:", "  name: Echo", "  type: http", "  seq: 1", "", "http:", "  method: GET", '  url: "{{baseUrl}}/echo/seq?token={{token}}"', ""].join("\n"),
  };
}

test("a diagram of what the core sent, without the query or the secret", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Sequence test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "Echo" }).click();
  // The secret is set through the app's own secret store.
  await page.evaluate(async (value) => {
    const b = (window as unknown as { __rt: { backend: { secrets: { set(env: string, name: string, v: string): Promise<void> } } } }).__rt.backend;
    await b.secrets.set("Local", "token", value);
  }, CANARY);
  for (let i = 0; i < 2; i++) {
    await page.getByTestId("send").click();
    await expect(page.getByTestId("response-status")).toHaveText("200");
  }

  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Sequence diagram: this session");
  await palette.getByRole("option").filter({ hasText: "Sequence diagram: this session" }).first().click();
  await expect(page.getByTestId("sequence-svg")).toBeVisible();
  await expect(page.getByTestId("sequence-participant").first()).toHaveText("Client");
  await expect(page.getByTestId("sequence-participant").nth(1)).toContainText("127.0.0.1");
  await expect(page.getByTestId("sequence-message").first()).toContainText("GET /echo/seq");
  await expect(page.getByTestId("sequence-message").nth(1)).toContainText(/^200 \(\d+ ms\)$|200 \(\d+ ms\)/);
  await expect(page.getByTestId("sequence-foot")).toContainText("4 messages, 2 participants");

  await page.getByTestId("sequence-copy").click();
  const copied = await page.evaluate(() => navigator.clipboard.readText());
  expect(copied.startsWith("```mermaid\nsequenceDiagram\n")).toBe(true);
  expect(copied).toContain("GET /echo/seq");
  expect(copied).not.toContain(CANARY);
  expect(copied).not.toContain("token=");
  expect(await page.getByTestId("sequence-svg").textContent()).not.toContain(CANARY);
});
