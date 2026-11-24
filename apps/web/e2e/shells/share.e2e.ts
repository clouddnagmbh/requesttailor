// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C: share a request with the real core (rt-wasm) on an OPFS folder. The link
// holds the request and no secret (canary in a header, the query and the body), making it
// calls nothing, and opening it in a fresh page offers it, then adds an unsaved draft.

import { expect, test, type Page, type Request } from "@playwright/test";

import { fakePicker, fillFolder, sampleCollection, SERVER } from "./env";

const CANARY = "RT-CANARY-shell-share-9f3a";
const MOD = process.platform === "darwin" ? "Meta" : "Control";

function collection(): Record<string, string> {
  return {
    ...sampleCollection(),
    "items.yml": [
      "info:",
      "  name: Items",
      "  type: http",
      "  seq: 5",
      "",
      "http:",
      "  method: POST",
      `  url: "{{baseUrl}}/echo/items?api_key=${CANARY}"`,
      "  headers:",
      "    - name: X-Api-Key",
      `      value: ${CANARY}`,
      "  body:",
      "    type: json",
      `    data: '{"password": "${CANARY}", "name": "x"}'`,
      "",
    ].join("\n"),
  };
}

async function runCommand(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const p = page.getByTestId("palette");
  await expect(p).toBeVisible();
  await page.getByTestId("palette-input").fill(label);
  await p.getByTestId("palette-option").filter({ hasText: label }).first().click();
  await expect(p).toBeHidden();
}

test("a link from the real core holds no secret, makes no request, and opens as a draft", async ({ page }) => {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("tree-request").filter({ hasText: "Items" }).click();

  const requests: Request[] = [];
  page.on("request", (r) => requests.push(r));
  await runCommand(page, "Share: link and QR code");
  const text = await page.getByTestId("share-link-text").innerText();
  expect(text).toMatch(/^http:\/\/127\.0\.0\.1:\d+\/#rt=1\./);
  expect(text).not.toContain(CANARY);
  await page.getByTestId("share-toggle-preview").click();
  const preview = await page.getByTestId("share-preview").innerText();
  expect(preview).not.toContain(CANARY);
  expect(preview).toContain("{{secret:api_key}}");
  await page.getByTestId("share-toggle-qr").click();
  await expect(page.getByTestId("share-qr").locator("svg")).toBeVisible();
  expect(requests.filter((r) => r.url().includes(SERVER) || r.url().includes("share")), "network log").toEqual([]);

  // As a snippet, variables stay variables.
  await page.getByTestId("share-tab-curl").click();
  const snippet = await page.getByTestId("share-snippet").innerText();
  expect(snippet).toContain("$BASE_URL");
  expect(snippet).not.toContain(CANARY);
  await page.getByTestId("share-dialog").getByRole("button", { name: "Close" }).click();

  // Opened from outside: a fresh page with the fragment offers it, and the user opens it.
  const fragment = text.slice(text.indexOf("#"));
  await page.goto("about:blank");
  await fakePicker(page);
  await page.goto(`/${fragment}`);
  await expect(page.getByTestId("shared-banner-summary")).toContainText("Items");
  await expect(page.getByTestId("shared-banner-summary")).not.toContainText(CANARY);
  expect(page.url()).not.toContain("#rt=");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await page.getByTestId("shared-open").click();
  await expect(page.getByTestId("shared-opened")).toContainText("is not saved and was not sent");
  await expect(page.getByTestId("url-input")).toContainText("{{baseUrl}}/echo/items");
  await expect(page.getByTestId("save")).toBeEnabled();
});
