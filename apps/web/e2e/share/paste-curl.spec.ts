// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C, mock backend: a pasted cURL command in the URL field gets a hint; Enter
// turns it into the request, Escape leaves the text. Nothing is sent.

import { expect, test, type Page } from "@playwright/test";

const CURL = "curl -X POST https://api.test/items -H 'Content-Type: application/json' -d '{\"a\":1}'";

async function open(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await expect(page.getByTestId("url-input")).toBeVisible();
}

/** A paste event with the text, as the browser delivers one to the field. */
async function paste(page: Page, text: string) {
  const field = page.getByTestId("url-input").locator(".cm-content");
  await field.click();
  await field.evaluate((el, value) => {
    const data = new DataTransfer();
    data.setData("text/plain", value);
    el.dispatchEvent(new ClipboardEvent("paste", { clipboardData: data, bubbles: true, cancelable: true }));
  }, text);
}

test("Enter turns a pasted cURL command into the request", async ({ page }) => {
  await open(page);
  const sent: string[] = [];
  page.on("request", (r) => r.method() === "POST" && sent.push(r.url()));
  await paste(page, CURL);
  const hint = page.getByTestId("paste-hint");
  await expect(hint).toContainText("This looks like a cURL command");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("paste-hint")).toHaveCount(0);
  await expect(page.getByTestId("url-input")).toContainText("https://api.test/items");
  await expect(page.getByTestId("method-select")).toHaveValue("POST");
  await expect(page.getByTestId("paste-note")).toContainText("cURL");
  expect(sent).toEqual([]);
});

test("Escape leaves the text as it is", async ({ page }) => {
  await open(page);
  await paste(page, CURL);
  await expect(page.getByTestId("paste-hint")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("paste-hint")).toHaveCount(0);
  await expect(page.getByTestId("method-select")).toHaveValue("GET");
});

test("the button takes it too, and an ordinary paste gets no hint", async ({ page }) => {
  await open(page);
  await paste(page, "https://plain.test/path");
  await expect(page.getByTestId("paste-hint")).toHaveCount(0);
  await paste(page, CURL);
  await page.getByTestId("paste-hint-take").click();
  await expect(page.getByTestId("url-input")).toContainText("https://api.test/items");
});

test("a pasted share link offers to open it as a draft", async ({ page }) => {
  await open(page);
  await paste(page, "rt:1.eyJ2ZXJzaW9uIjoxfQ");
  await expect(page.getByTestId("paste-hint")).toContainText("shared request");
});
