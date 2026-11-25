// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 19, strand R (passive security checks): a finding shows in the same
// checks chip and tab as the checks of wave 18, tagged with its OWASP API Security Top 10
// category, and can be switched off like any check. Mock backend; the rules themselves are
// tested in the core (`crates/rt-app/tests/security_passive.rs`).

import { expect, test, type Page } from "@playwright/test";

async function sendList(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  await expect(page.getByTestId("checks-panel")).toBeVisible();
}

test("a security rule's observation carries its OWASP category", async ({ page }) => {
  await sendList(page);
  const item = page.locator('[data-testid=checks-item][data-id="chk/nosniff-missing"]');
  await expect(item).toBeVisible();
  const tag = item.getByTestId("checks-owasp");
  await expect(tag).toHaveText("API8");
  await expect(tag).toHaveAttribute("data-category", "API8:2023");
  await expect(tag).toHaveAttribute("title", /Security Misconfiguration/);
});

test("an observation of another rule has no tag", async ({ page }) => {
  await sendList(page);
  // Cookie flags belong to the checks of wave 18, not to the security audit.
  const others = page.locator('[data-testid=checks-item]:not([data-id^="chk/nosniff"]):not([data-id^="chk/hsts"]):not([data-id^="chk/cors"]):not([data-id^="chk/stack"]):not([data-id^="chk/version"]):not([data-id^="chk/jwt"])');
  for (const item of await others.all()) await expect(item.getByTestId("checks-owasp")).toHaveCount(0);
});
