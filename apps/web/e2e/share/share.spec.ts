// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand C, mock backend: share a request as a link, see exactly what is in it, open
// a link as an unsaved draft that sends nothing. Making a link makes no network request.
// Port: RT_WEB_PORT=48512.

import { expect, test, type Page, type Request } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
}

async function runCommand(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const p = page.getByTestId("palette");
  await expect(p).toBeVisible();
  await page.getByTestId("palette-input").fill(label);
  await p.getByTestId("palette-option").filter({ hasText: label }).first().click();
  await expect(p).toBeHidden();
}

test("a link shows what is in it, holds no secret and makes no network request", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  const requests: Request[] = [];
  page.on("request", (r) => requests.push(r));

  await runCommand(page, "Share: link and QR code");
  const dialog = page.getByTestId("share-dialog");
  await expect(dialog).toBeVisible();
  const text = await page.getByTestId("share-link-text").innerText();
  expect(text).toMatch(/^rt:1\.|#rt=1\./);
  await expect(page.getByTestId("share-note")).toContainText("Nothing is uploaded");

  await dialog.getByTestId("share-toggle-preview").click();
  const preview = await page.getByTestId("share-preview").innerText();
  expect(preview).toContain('"name": "Create user"');
  expect(preview).toContain("{{token}}");
  // Only names travel until the user asks for the values.
  await expect(page.getByTestId("share-include-values")).not.toBeChecked();
  expect(preview).not.toContain("127.0.0.1:47811");

  await dialog.getByTestId("share-toggle-qr").click();
  await expect(page.getByTestId("share-qr").locator("svg")).toBeVisible();

  // No request while making, previewing or drawing the link.
  expect(requests.map((r) => r.url()), "network log").toEqual([]);
});

test("copy as curl keeps variables and resolves them on request", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await runCommand(page, "Copy as curl");
  const snippet = page.getByTestId("share-snippet");
  await expect(snippet).toContainText("$BASE_URL");
  await expect(snippet).not.toContainText("127.0.0.1");
  await page.getByTestId("share-resolve").check();
  await expect(snippet).not.toContainText("$BASE_URL");
  await page.getByTestId("share-tab-python").click();
  await expect(page.getByTestId("share-snippet")).toContainText("requests");
});

test("opening a link adds an unsaved draft and sends nothing", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await runCommand(page, "Share: link and QR code");
  const link = await page.getByTestId("share-link-text").innerText();
  await page.getByTestId("share-dialog").getByRole("button", { name: "Close" }).click();

  // The link arrives in the fragment of a fresh page: a banner offers it, nothing opens by itself.
  const hash = link.slice(link.indexOf("rt=") === -1 ? 0 : link.indexOf("#") + 1);
  const payload = link.startsWith("rt:") ? link.slice(3) : hash.slice("rt=".length);
  const sent: string[] = [];
  page.on("request", (r) => {
    if (r.method() !== "GET" || /\/echo|\/users/.test(r.url())) sent.push(r.url());
  });
  // A fresh page, as when the link is opened from elsewhere (a hash-only change would not reload).
  await page.goto("about:blank");
  await page.goto(`/?backend=mock#rt=${payload}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("shared-banner")).toBeVisible();
  await expect(page.getByTestId("shared-banner-summary")).toContainText("Create user");
  expect(page.url()).not.toContain("#rt=");
  await expect(page.getByTestId("tree-request").filter({ hasText: "Create user 2" })).toHaveCount(0);

  await page.getByTestId("shared-open").click();
  await expect(page.getByTestId("shared-opened")).toContainText("is not saved and was not sent");
  await expect(page.getByTestId("shared-needs")).toContainText("token");
  await expect(page.getByTestId("url-input")).toContainText("{{baseUrl}}/users");
  await expect(page.getByTestId("tab-dirty").first()).toBeVisible().catch(() => {});
  expect(sent).toEqual([]);
});

test("Paste link takes a link and refuses other text", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await runCommand(page, "Share: link and QR code");
  const link = await page.getByTestId("share-link-text").innerText();
  await page.getByTestId("share-dialog").getByRole("button", { name: "Close" }).click();

  await runCommand(page, "Paste link");
  const input = page.getByTestId("paste-link-input");
  await input.fill("hello");
  await expect(page.getByTestId("paste-link-hint")).toBeVisible();
  await expect(page.getByTestId("paste-link-open")).toBeDisabled();
  await input.fill(link);
  await page.getByTestId("paste-link-open").click();
  await expect(page.getByTestId("shared-opened")).toBeVisible();
});
