// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand G: the secret rescue wizard against the mock backend (`?backend=mock`,
// apps/ui/src/commands/mock-rescue.ts): nothing is rewritten without the confirmation, the
// force-push warning and the hint to rotate are shown afterwards, the value only ends up in
// the secret store, never on the page.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type W = Window & { __rt: { backend: MockBackend } };

const MOD = "ControlOrMeta";
// apps/ui/src/commands/mock-rescue.ts, assembled the same way.
const VALUE = ["ghp_", "M0ck", "Rescue", "Value", "0123456789abcdefABCDEF"].join("");

async function open(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("secrets");
  await dialog.getByRole("option").filter({ hasText: "Scan history for secrets" }).click();
  await expect(page.getByTestId("git-view")).toHaveAttribute("data-kind", "rescue");
}

const stored = (page: Page) =>
  page.evaluate(() => (window as unknown as W).__rt.backend.secrets.get("Local", "githubToken"));

test("rewrites only after the confirmation, then warns about the force push and says to rotate", async ({ page }) => {
  await open(page);
  const wizard = page.getByTestId("rescue-wizard");
  await page.getByTestId("rescue-scan").click();
  const findings = page.getByTestId("rescue-finding");
  await expect(findings).toHaveCount(2);
  await expect(findings.nth(0)).toContainText("GitHub token");
  await expect(findings.nth(0)).toContainText("users/list-users.yml:9");
  await expect(findings.nth(0)).toContainText("value: Bearer ***");
  await expect(findings.nth(0)).toContainText("in HEAD");
  // A private key cannot become a variable.
  await expect(findings.nth(1).getByTestId("rescue-choose")).toHaveCount(0);
  await expect(findings.nth(1)).toContainText("git filter-repo");
  await page.screenshot({ path: test.info().outputPath("rescue-findings.png"), fullPage: true });

  await findings.nth(0).getByTestId("rescue-choose").click();
  await expect(page.getByTestId("rescue-variable")).toHaveValue("githubToken");
  await expect(page.getByTestId("rescue-environment")).toHaveValue("Local");
  await expect(page.getByTestId("rescue-explain")).toContainText("refs/requesttailor/backup/");
  await expect(page.getByTestId("rescue-explain")).toContainText("{{githubToken}}");

  // Without the confirmation nothing can be rewritten.
  const rewrite = page.getByTestId("rescue-rewrite");
  await expect(rewrite).toBeDisabled();
  await rewrite.click({ force: true });
  await expect(page.getByTestId("rescue-done")).toHaveCount(0);
  expect(await stored(page)).toBeNull();
  // An invalid variable blocks it too.
  await page.getByTestId("rescue-confirm").check();
  await page.getByTestId("rescue-variable").fill("not a name");
  await expect(rewrite).toBeDisabled();
  await page.getByTestId("rescue-variable").fill("githubToken");
  await expect(rewrite).toBeEnabled();
  await page.screenshot({ path: test.info().outputPath("rescue-plan.png"), fullPage: true });

  await rewrite.click();
  await expect(page.getByTestId("rescue-done")).toContainText("{{githubToken}}");
  const push = page.getByTestId("rescue-force-push");
  await expect(push).toBeVisible();
  await expect(push).toContainText("git push --force-with-lease origin main");
  await expect(push).not.toContainText("--force-with-lease origin feature");
  await expect(page.getByTestId("rescue-rotate")).toContainText("Rotate the secret");
  await expect(page.getByTestId("rescue-rotate")).toContainText("Environments → Local → githubToken");
  await expect(page.getByTestId("rescue-tags")).toContainText("v1.0");
  await page.screenshot({ path: test.info().outputPath("rescue-done.png"), fullPage: true });

  // The value is in the secret store, never on the page.
  expect(await stored(page)).toBe(VALUE);
  expect(await page.content()).not.toContain(VALUE);
  // `Local` declares `githubToken` already as a secret? The mock's has `token`: it was added.
  await expect(page.getByTestId("rescue-declared")).toContainText("githubToken");
  const env = await page.evaluate(async () =>
    (await (window as unknown as W).__rt.backend.environments()).find((e) => e.name === "Local"),
  );
  expect(env?.variables.find((v) => v.name === "githubToken")).toMatchObject({ secret: true, value: null });

  // Scanned again: only the private key is left.
  await wizard.getByTestId("rescue-rescan").click();
  await expect(findings).toHaveCount(1);
  await expect(findings.nth(0)).toContainText("private key");
});

test("unsaved changes block the rewrite", async ({ page }) => {
  await open(page);
  await page.evaluate(async () => {
    await (window as unknown as W).__rt.backend.createRequest("", "Draft");
  });
  // The app's state learns about the edit when it reloads; use the UI's own path instead.
  await page.getByTestId("rescue-scan").click();
  await page.getByTestId("rescue-choose").first().click();
  await page.getByTestId("rescue-confirm").check();
  await page.getByTestId("rescue-rewrite").click();
  await expect(page.getByTestId("rescue-error")).toContainText("Save or discard your changes");
  expect(await stored(page)).toBeNull();
});
