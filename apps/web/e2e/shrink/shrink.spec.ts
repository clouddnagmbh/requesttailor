// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand E: the minimal failing request against the mock backend (`?backend=mock`). The
// mock plays a server that fails (500) while the request holds the text `boom`; the search keeps
// that and takes everything else away. A writing request asks first and is never shrunk against
// production; the result opens as an unsaved request and copies as curl and as a link.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const LIST = "users/list-users.yml";

type Spec = { url: string; method: string; headers: { name: string; value: string; enabled: boolean }[] };
type Api = { getRequest(p: string): Promise<Spec>; updateRequest(p: string, s: Spec): Promise<void> };

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

/** Gives the request many parts and one that makes the mock server fail. */
async function failing(page: Page, path: string) {
  await page.evaluate(async (p) => {
    const b = (window as unknown as { __rt: { backend: Api } }).__rt.backend;
    const spec = await b.getRequest(p);
    spec.url = "{{baseUrl}}/users?page=1&debug=1&boom=1&sort=name&x=2";
    spec.headers = [
      { name: "Accept", value: "application/json", enabled: true },
      { name: "X-Debug", value: "1", enabled: true },
      { name: "X-Trace", value: "abc", enabled: true },
      { name: "Authorization", value: "Bearer {{token}}", enabled: true },
    ];
    await b.updateRequest(p, spec);
  }, path);
}

async function openShrink(page: Page, name: string) {
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Shrink failing");
  await palette.getByRole("option").filter({ hasText: "Shrink failing request" }).click();
  await expect(page.getByTestId("shrink-panel")).toBeVisible();
}

test("the palette opens the view; a reading request is shrunk to what makes it fail", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await failing(page, LIST);
  await openShrink(page, "List users");
  await expect(page.getByTestId("shrink-target")).toContainText("users?page=1&debug=1&boom=1");
  // A reading request asks nothing.
  await expect(page.getByTestId("shrink-writes")).toHaveCount(0);
  await page.getByTestId("shrink-start").click();

  const result = page.getByTestId("shrink-result");
  await expect(result).toBeVisible();
  await expect(result).toHaveAttribute("data-outcome", "found");
  await expect(page.getByTestId("shrink-removed")).toContainText("X-Debug");
  await expect(page.getByTestId("shrink-removed")).toContainText("debug");
  await expect(page.getByTestId("shrink-removed")).toContainText("sort");
  await expect(page.getByTestId("shrink-removed")).not.toContainText("boom");
  // The cause and the authorization stay.
  await expect(page.getByTestId("shrink-smallest")).toContainText("users?boom=1");
  await expect(page.getByTestId("shrink-kept-headers")).toHaveText("Authorization");
  await expect(page.getByTestId("shrink-note").first()).toContainText("1-minimal");
  // The numbers are on the result.
  expect(Number(await result.getAttribute("data-probes"))).toBeLessThan(40);
});

test("keep and the probe limit are honoured", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await failing(page, LIST);
  await openShrink(page, "List users");
  await page.getByTestId("shrink-keep").fill("header:X-Trace");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-kept-headers")).toHaveText("X-Trace, Authorization");

  await page.getByTestId("shrink-max").fill("3");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-result")).toHaveAttribute("data-outcome", "stopped");
  await expect(page.getByTestId("shrink-note").first()).toContainText("limit of probes");
});

test("a failure that does not show says so", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await openShrink(page, "Health");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-result")).toHaveAttribute("data-outcome", "not-reproduced");
  await expect(page.getByTestId("shrink-result")).toContainText("did not show again");
});

test("a bad value is explained and nothing is sent", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await openShrink(page, "Health");
  await page.getByTestId("shrink-predicate").selectOption("status");
  await page.getByTestId("shrink-value").fill("abc");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-error")).toContainText("between 100 and 599");
  await expect(page.getByTestId("shrink-result")).toHaveCount(0);
});

test("the result opens as an unsaved request and copies as curl and as a link, without a secret", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await failing(page, LIST);
  await openShrink(page, "List users");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-result")).toBeVisible();

  await page.getByTestId("shrink-copy-curl").click();
  await expect(page.getByTestId("shrink-done")).toContainText("curl command copied");
  const curl = await page.evaluate(() => navigator.clipboard.readText());
  expect(curl).toContain("curl");
  expect(curl).toContain("boom=1");
  expect(curl).not.toContain("X-Debug");
  expect(curl).not.toMatch(/Bearer [A-Za-z0-9]/);

  await page.getByTestId("shrink-copy-link").click();
  await expect(page.getByTestId("shrink-done")).toContainText("Link copied");
  const link = await page.evaluate(() => navigator.clipboard.readText());
  expect(link).toMatch(/rt[:=#]/);

  // Copying left the original request as it was.
  expect(await page.evaluate(() => (window as unknown as { __rt: { backend: Api } }).__rt.backend.getRequest("users/list-users.yml").then((r) => r.url))).toContain("debug=1");
  // One draft, opened: the tree has the new request, the view is gone.
  await page.getByTestId("shrink-open-draft").click();
  await expect(page.getByTestId("shrink-panel")).toHaveCount(0);
  await expect(page.getByTestId("tree-request").filter({ hasText: "Minimal: List users" })).toHaveCount(1);
  await expect(page.getByTestId("url-input")).toContainText("boom=1");
});

test("a writing request asks first, and is not shrunk against production", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: Api } }).__rt.backend;
    const spec = await b.getRequest("users/create-user.yml");
    spec.url = "{{baseUrl}}/users?boom=1&x=2";
    await b.updateRequest("users/create-user.yml", spec);
  });
  await openShrink(page, "Create user");
  await expect(page.getByTestId("shrink-writes")).toContainText("This sends up to 100 requests that write.");
  // Esc on the question sends nothing.
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-confirm-writes")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("shrink-confirm-writes")).toBeHidden();
  await expect(page.getByTestId("shrink-result")).toHaveCount(0);
  // Confirmed: it runs.
  await page.getByTestId("shrink-start").click();
  await page.getByTestId("shrink-confirm-writes-yes").click();
  await expect(page.getByTestId("shrink-result")).toHaveAttribute("data-outcome", "found");

  // Production: no switch, no question, the button is off.
  await page.getByTestId("env-switcher").selectOption("PRD");
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Shrink failing");
  await palette.getByRole("option").filter({ hasText: "Shrink failing request" }).click();
  await expect(page.getByTestId("shrink-blocked")).toContainText("never shrunk against PRD");
  await expect(page.getByTestId("shrink-start")).toBeDisabled();
});

test("production asks once for a read", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await failing(page, LIST);
  await openShrink(page, "List users");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-confirm-production")).toBeVisible();
  await page.getByTestId("shrink-confirm-production-cancel").click();
  await expect(page.getByTestId("shrink-result")).toHaveCount(0);
  await page.getByTestId("shrink-start").click();
  await page.getByTestId("shrink-confirm-production-yes").click();
  await expect(page.getByTestId("shrink-result")).toHaveAttribute("data-outcome", "found");
});

test("no horizontal scrolling at 900 px", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await failing(page, LIST);
  await openShrink(page, "List users");
  await page.getByTestId("shrink-start").click();
  await expect(page.getByTestId("shrink-result")).toBeVisible();
  const overflow = await page.getByTestId("shrink-panel").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});
