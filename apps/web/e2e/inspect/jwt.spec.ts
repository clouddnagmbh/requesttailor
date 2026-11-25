// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand B (point 2): the JWT view (palette "Inspect token…": claims,
// date and relative time, a countdown, "signature not verified") and the word before a send with
// an expired token (Enter sends, Escape cancels, can be switched off). Mock backend.

import { expect, test, type Page } from "@playwright/test";

const b64 = (v: unknown) => Buffer.from(JSON.stringify(v)).toString("base64url");
const token = (claims: Record<string, unknown>, alg = "RS256") => `${b64({ alg, typ: "JWT" })}.${b64(claims)}.c2lnbmF0dXJl`;
const now = () => Math.floor(Date.now() / 1000);

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function inspectToken(page: Page, value: string) {
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Inspect token");
  await palette.getByRole("option").filter({ hasText: "Inspect token" }).click();
  await expect(page.getByTestId("jwt-view")).toBeVisible();
  await page.getByTestId("jwt-input").fill(value);
}

test("the view shows header, claims with their meaning, times as date and relative time", async ({ page }) => {
  await openDemo(page);
  await inspectToken(page, token({ iss: "https://idp.example", sub: "ada", exp: now() + 3700, iat: now() - 60, scope: "read write" }));
  await expect(page.getByTestId("jwt-unverified")).toContainText("Signature not verified");
  await expect(page.getByTestId("jwt-header")).toContainText('"alg": "RS256"');
  const claims = page.getByTestId("jwt-claims");
  await expect(claims.locator('[data-claim="iss"]')).toContainText("https://idp.example");
  await expect(claims.locator('[data-claim="iss"]')).toContainText("Issuer");
  await expect(claims.locator('[data-claim="exp"]')).toContainText(/\d{4}-\d{2}-\d{2}T.*Z/);
  await expect(claims.locator('[data-claim="exp"]')).toContainText("(in 1 h)");
  await expect(claims.locator('[data-claim="iat"]')).toContainText("(1 min ago)");
  await expect(claims.locator('[data-claim="scope"]')).toContainText("read");
  await expect(page.getByTestId("jwt-warning")).toHaveCount(0);
});

test("the countdown ticks while the view is open", async ({ page }) => {
  await openDemo(page);
  await inspectToken(page, token({ exp: now() + 100 }));
  const countdown = page.getByTestId("jwt-countdown");
  await expect(countdown).toContainText(/expires in 1 min \d+ s/);
  const first = await countdown.innerText();
  await expect.poll(async () => countdown.innerText(), { timeout: 5000 }).not.toBe(first);
});

test("an expired token is said so, a broken one is explained, and the value is not kept", async ({ page }) => {
  await openDemo(page);
  await inspectToken(page, token({ sub: "RT-CANARY-view", exp: now() - 720 }));
  await expect(page.getByTestId("jwt-warning")).toContainText("Token expired 12 min ago.");
  await expect(page.getByTestId("jwt-countdown")).toContainText("expired 12 min ago");
  await page.getByTestId("jwt-input").fill("not a token");
  await expect(page.getByTestId("jwt-error")).toBeVisible();
  await page.getByTestId("jwt-close").click();
  await expect(page.getByTestId("jwt-view")).toHaveCount(0);
  // Nothing of the token is in the page's storage.
  const stored = await page.evaluate(() => JSON.stringify([{ ...localStorage }, { ...sessionStorage }]));
  expect(stored).not.toContain("RT-CANARY-view");
});

async function withBearer(page: Page, bearer: string) {
  await openDemo(page);
  await page.evaluate(async (value) => {
    const path = "users/list-users.yml";
    const spec = await window.__rt.backend.getRequest(path);
    await window.__rt.backend.updateRequest(path, { ...spec, headers: [...spec.headers, { name: "Authorization", value: `Bearer ${value}`, enabled: true }] });
  }, bearer);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
}

const warning = (page: Page) => page.getByTestId("token-warning");

test("an expired token asks first: Escape cancels, Enter sends", async ({ page }) => {
  await withBearer(page, token({ exp: now() - 720 }));
  await page.getByTestId("send").click();
  await expect(warning(page)).toBeVisible();
  await expect(page.getByTestId("token-warning-text")).toHaveText("Token expired 12 min ago.");
  await expect(warning(page)).toContainText("Authorization header");
  await expect(page.getByTestId("response-status")).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(warning(page)).toHaveCount(0);
  await expect(page.getByTestId("response-status")).toHaveCount(0);

  await page.getByTestId("send").click();
  await expect(warning(page)).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(warning(page)).toHaveCount(0);
  await expect(page.getByTestId("response-status")).toBeVisible();
});

test("a good token and a request without one go out without a question", async ({ page }) => {
  await withBearer(page, token({ exp: now() + 3600 }));
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await expect(warning(page)).toHaveCount(0);
});

test("the warning can be switched off on this device", async ({ page }) => {
  await withBearer(page, token({ exp: now() - 5 }));
  await page.getByTestId("send").click();
  await page.getByTestId("token-warning-never").check();
  await page.getByTestId("token-warning-send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await page.getByTestId("send").click();
  await expect(warning(page)).toHaveCount(0);
});

test("a token in the request can be inspected from the Checks tab", async ({ page }) => {
  await withBearer(page, token({ iss: "https://idp.example", exp: now() + 3600 }));
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  const found = page.getByTestId("checks-token");
  await expect(found).toHaveCount(1);
  await expect(found).toContainText("Authorization header");
  await found.getByTestId("checks-inspect-token").click();
  await expect(page.getByTestId("jwt-claims").locator('[data-claim="iss"]')).toContainText("https://idp.example");
});
