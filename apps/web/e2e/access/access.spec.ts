// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand D: the two-user access check against the mock backend (`?backend=mock`). The
// mock plays a server with owners: user B gets A's object when the URL says `leak`, a 500 for
// `crash`, and a 403 otherwise. A reading request runs without a question; a writing request
// asks "Allow writes for this check?" and is never checked against production.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const LIST = "users/list-users.yml";
const CREATE = "users/create-user.yml";

type Spec = { url: string; method: string };
type Api = {
  getRequest(p: string): Promise<Spec>;
  updateRequest(p: string, s: Spec): Promise<void>;
  agents: { policy(): Promise<{ access: string; environments: unknown[] }>; setPolicy(p: unknown): Promise<void> };
};

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("env-switcher").selectOption("Local");
}

async function setUrl(page: Page, path: string, url: string) {
  await page.evaluate(
    async ([p, u]) => {
      const b = (window as unknown as { __rt: { backend: Api } }).__rt.backend;
      const spec = await b.getRequest(p!);
      spec.url = u!;
      await b.updateRequest(p!, spec);
    },
    [path, url],
  );
}

async function openAccess(page: Page, name: string) {
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Check access");
  await palette.getByRole("option").filter({ hasText: "Check access" }).click();
  await expect(page.getByTestId("access-panel")).toBeVisible();
}

test("a reading request that leaks is broken object access, with evidence and no values", async ({ page }) => {
  await openDemo(page);
  await setUrl(page, LIST, "{{baseUrl}}/leak/{{orderId}}");
  await openAccess(page, "List users");
  await expect(page.getByTestId("access-target")).toContainText("leak/{{orderId}}");
  // A is the selected environment, B another one; reads ask nothing.
  await expect(page.getByTestId("access-a")).toHaveValue("Local");
  await expect(page.getByTestId("access-b")).toHaveValue("PRD");
  await expect(page.getByTestId("access-writes")).toHaveCount(0);
  await page.getByTestId("access-start").click();

  const result = page.getByTestId("access-result");
  await expect(result).toHaveAttribute("data-verdict", "broken-object-access");
  await expect(result).toHaveAttribute("data-broken", "true");
  await expect(result).toContainText("PRD can read what belongs to Local");
  await expect(page.getByTestId("access-finding")).toContainText("B received object 1");
  await expect(page.getByTestId("access-status-A")).toHaveText("200");
  await expect(page.getByTestId("access-status-B")).toHaveText("200");
  await expect(page.getByTestId("access-evidence")).toContainText("$.id");
  await expect(page.getByTestId("access-evidence")).toContainText("string");
});

test("a denied B is ok, a crash is inconclusive", async ({ page }) => {
  await openDemo(page);
  await setUrl(page, LIST, "{{baseUrl}}/orders/{{orderId}}");
  await openAccess(page, "List users");
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "ok");
  await expect(page.getByTestId("access-status-B")).toHaveText("403");
  await setUrl(page, LIST, "{{baseUrl}}/crash/{{orderId}}");
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "inconclusive");
});

test("without an object reference nothing is guessed, and says what to do", async ({ page }) => {
  await openDemo(page);
  await openAccess(page, "Health");
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "skipped");
  await expect(page.getByTestId("access-hint")).toContainText("{{orderId}}");
});

test("B must be denied checks a function without ids", async ({ page }) => {
  await openDemo(page);
  await setUrl(page, "health.yml", "{{baseUrl}}/leak/admin");
  await openAccess(page, "Health");
  await page.getByTestId("access-denied").check();
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "broken-function-access");
});

test("a writing request asks first and is never checked against production", async ({ page }) => {
  await openDemo(page);
  await setUrl(page, CREATE, "{{baseUrl}}/leak/{{orderId}}");
  // Production: no question, the button is off.
  await openAccess(page, "Create user");
  await expect(page.getByTestId("access-blocked")).toContainText("never checked against a production environment (PRD)");
  await expect(page.getByTestId("access-start")).toBeDisabled();

  // Say PRD is not production (the policy names it), and the question appears.
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: Api } }).__rt.backend;
    const policy = await b.agents.policy();
    await b.agents.setPolicy({ ...policy, environments: [{ environment: "PRD", access: "write", production: false }] });
  });
  await page.getByRole("button", { name: "Close Check access" }).click();
  await expect(page.getByTestId("access-panel")).toHaveCount(0);
  await openAccess(page, "Create user");
  await expect(page.getByTestId("access-writes-note")).toBeVisible();
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-confirm-writes")).toBeVisible();
  // Esc sends nothing.
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("access-confirm-writes")).toBeHidden();
  await expect(page.getByTestId("access-result")).toHaveCount(0);
  // Confirmed: it runs.
  await page.getByTestId("access-start").click();
  await page.getByTestId("access-confirm-writes-yes").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "broken-object-access");
});
