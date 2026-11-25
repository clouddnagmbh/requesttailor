// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand F: consumer contracts against the mock backend (`?backend=mock`). The mock lists
// three consumers and plays the provider: a base URL that holds `break` makes `billing` break at
// `GET /orders/{orderId}`. The view names the consumer, the request and the assertion, leaves
// writing requests out without "Allow writes", and shows nothing of the answer.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openConsumers(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Verify consumers");
  await palette.getByRole("option").filter({ hasText: "Verify consumers" }).click();
  await expect(page.getByTestId("consumers-panel")).toBeVisible();
}

test("lists the consumers and verifies a provider that kept its promise", async ({ page }) => {
  await openConsumers(page);
  await expect(page.getByTestId("consumer-row")).toHaveCount(3);
  await expect(page.getByTestId("consumer-row").filter({ hasText: "mobile" })).toContainText("main");
  // Nothing is sent before the button.
  await expect(page.getByTestId("consumers-report")).toHaveCount(0);
  await page.getByTestId("consumers-verify").click();
  await expect(page.getByTestId("consumers-summary")).toHaveText("0 of 3 consumers broken");
  await expect(page.getByTestId("consumer-failure")).toHaveCount(0);
  await expect(page.getByTestId("consumer-result").filter({ hasText: "mobile" })).toContainText("commit 0123456");
});

test("a breaking consumer is named with the request, the assertion and the difference", async ({ page }) => {
  await openConsumers(page);
  await page.getByTestId("consumers-base-url").fill("http://break.test");
  await page.getByTestId("consumers-verify").click();
  await expect(page.getByTestId("consumers-summary")).toHaveText("1 of 3 consumers broken");
  const billing = page.getByTestId("consumer-result").filter({ hasText: "billing breaks" });
  await expect(billing).toHaveAttribute("data-broken", "true");
  const failure = billing.getByTestId("consumer-failure");
  await expect(failure).toContainText("/orders/{orderId}");
  await expect(failure).toContainText("res.body.items[0].price isNumber");
  await expect(failure).toContainText("got string");
  await expect(page.getByTestId("consumer-result").filter({ hasText: "portal is fine" })).toBeVisible();
});

test("writing requests are left out until writes are allowed", async ({ page }) => {
  await openConsumers(page);
  await expect(page.getByTestId("consumers-writes-hint")).toContainText("never against an environment that looks like production");
  await page.getByTestId("consumers-verify").click();
  await expect(page.getByTestId("consumer-result").filter({ hasText: "billing" })).toContainText("1 left out");
  await page.getByTestId("consumers-writes").check();
  await page.getByTestId("consumers-verify").click();
  await expect(page.getByTestId("consumer-result").filter({ hasText: "billing" })).not.toContainText("left out");
  await expect(page.getByTestId("consumer-result").filter({ hasText: "billing" })).toContainText("2 requests");
});

test("only the chosen consumers run", async ({ page }) => {
  await openConsumers(page);
  await page.getByTestId("consumer-row").filter({ hasText: "billing" }).getByRole("checkbox").uncheck();
  await page.getByTestId("consumer-row").filter({ hasText: "mobile" }).getByRole("checkbox").uncheck();
  await page.getByTestId("consumers-verify").click();
  await expect(page.getByTestId("consumer-result")).toHaveCount(1);
  await expect(page.getByTestId("consumers-summary")).toHaveText("0 of 1 consumer broken");
});
