// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand D: the cookie dialog (mock backend). A response's `Set-Cookie` lands in the
// jar of the active environment; the dialog lists it, deletes one and clears all, and other
// environments have their own jar.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

/** Sends a new request to `/cookies/set?<query>` in `environment` straight through the mock. */
async function setCookies(page: Page, environment: string, query: string) {
  await page.evaluate(
    async ({ environment, query }) => {
      const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").MockBackend } }).__rt.backend;
      const path = await b.createRequest("", `Set ${query}`);
      const spec = await b.getRequest(path);
      await b.updateRequest(path, { ...spec, url: `https://api.example.com/cookies/set?${query}` });
      await b.send(path, { environment, vars: [] });
    },
    { environment, query },
  );
}

async function palette(page: Page, filter: string, option: string) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(filter);
  await dialog.getByRole("option").filter({ hasText: option }).click();
}

async function openCookies(page: Page) {
  await palette(page, "cookies", "Show cookies");
  await expect(page.getByTestId("cookies-dialog")).toBeVisible();
}

test("cookies of the active environment are listed, deleted and cleared", async ({ page }) => {
  await openCollection(page);
  await expect(page.getByTestId("env-active")).toHaveText("Local");
  await setCookies(page, "Local", "session=abc&theme=dark");

  await openCookies(page);
  const rows = page.getByTestId("cookie-row");
  // By the name cell: session cookies also say "Session" in the expiry column.
  const row = (name: string) =>
    rows.filter({ has: page.getByTestId("cookie-name").getByText(name, { exact: true }) });
  await expect(rows).toHaveCount(2);
  await expect(row("session").getByTestId("cookie-value")).toHaveText("abc");
  await expect(rows.first().getByTestId("cookie-domain")).toHaveText("api.example.com");

  await row("theme").getByTestId("cookie-delete").click();
  await expect(rows).toHaveCount(1);

  await page.getByTestId("cookies-clear").click();
  await page.getByTestId("cookies-clear-confirm").click();
  await expect(rows).toHaveCount(0);
  await expect(page.getByTestId("cookies-empty")).toHaveText("No cookies stored for this environment.");
  await page.getByTestId("cookies-close").click();
  await expect(page.getByTestId("cookies-dialog")).toBeHidden();
});

test("each environment has its own jar", async ({ page }) => {
  await openCollection(page);
  await setCookies(page, "PRD", "prd=1");
  await openCookies(page);
  await expect(page.getByTestId("cookies-environment")).toHaveText("Local");
  await expect(page.getByTestId("cookies-empty")).toBeVisible();
  await page.getByTestId("cookies-close").click();

  await palette(page, "Environment: PRD", "Environment: PRD");
  await expect(page.getByTestId("env-active")).toHaveText("PRD");

  await openCookies(page);
  await expect(page.getByTestId("cookie-row")).toHaveCount(1);
  await expect(page.getByTestId("cookie-name")).toHaveText("prd");
});
