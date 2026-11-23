// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand E against the mock backend (`?backend=mock`): the commit dialog blocks a
// commit with a plaintext token and shows file, line and hint; the environment editor edits
// external secret references without a value field. Fake tokens are assembled at run time.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Environment, MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const TOKEN = ["9c1e2f3a4b5c", "6d7e8f90a1b2"].join("");

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const git = (window as unknown as W).__rt.backend.git;
    git.scenario("clean");
    await git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
  });
}

async function setText(field: Locator, text: string) {
  const content = field.locator(".cm-content");
  await content.click();
  await content.press("ControlOrMeta+a");
  await content.page().keyboard.type(text);
}

/** Sets the Authorization header of "Health" in the editor and saves. */
async function setAuthorization(page: Page, value: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("tab-headers").click();
  const table = page.getByTestId("headers-table");
  if ((await table.getByTestId("kv-row").count()) === 0) await table.getByTestId("kv-add").click();
  const row = table.getByTestId("kv-row").nth(0);
  await setText(row.getByTestId("kv-name"), "Authorization");
  await setText(row.getByTestId("kv-value"), value);
  await expect(page.getByTestId("save")).toBeEnabled();
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
}

const gitCalls = (page: Page) => page.evaluate(() => [...(window as unknown as W).__rt.backend.git.calls]);

test("the commit dialog blocks a plaintext token and shows file, line and hint", async ({ page }) => {
  await openDemo(page);
  await setAuthorization(page, `Bearer ${TOKEN}`);
  await page.getByTestId("view-git").click();
  await expect(page.locator('[data-testid="git-file"][data-path="health.yml"]')).toBeVisible();

  await page.getByTestId("git-message").fill("Add auth");
  await page.getByTestId("git-commit").click();
  const panel = page.getByTestId("git-secrets");
  await expect(panel).toBeVisible();
  await expect(panel.getByRole("heading")).toHaveText("Commit blocked: 1 credential in 1 file");
  const finding = panel.getByTestId("git-secret-finding");
  await expect(finding).toHaveCount(1);
  await expect(finding.getByTestId("git-secret-location")).toHaveText(/^health\.yml:\d+$/);
  await expect(finding.getByTestId("git-secret-kind")).toHaveText("bearer credential");
  await expect(finding.getByTestId("git-secret-snippet")).toContainText("***");
  await expect(finding.getByTestId("git-secret-hint")).toContainText("secret variable");
  await expect(finding.getByTestId("git-secret-hint")).toContainText("{{token}}");
  await expect(page.getByTestId("git-secrets")).not.toContainText(TOKEN);

  // Nothing was committed; the file is still changed and the message kept.
  expect(await gitCalls(page)).not.toContain("commit health.yml");
  await expect(page.getByTestId("git-commit-subject")).toHaveText(["Initial collection"]);
  await expect(page.getByTestId("git-message")).toHaveValue("Add auth");
  // There is no way around it in the dialog.
  await expect(page.getByRole("button", { name: /anyway/i })).toHaveCount(0);

  // With a variable instead, the findings go away and the commit goes through.
  await setAuthorization(page, "Bearer {{token}}");
  await page.getByTestId("view-git").click();
  await expect(page.getByTestId("git-secrets")).toHaveCount(0);
  await page.getByTestId("git-message").fill("Add auth");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-commit-subject").first()).toContainText("Add auth");
  await expect(page.getByTestId("git-secrets")).toHaveCount(0);
});

test("a commit the backend refuses with secrets-found shows the findings too", async ({ page }) => {
  await openDemo(page);
  await setAuthorization(page, `Bearer ${TOKEN}`);
  // A scan that misses it (e.g. the file changed in between): the commit still refuses.
  await page.evaluate(() => {
    const git = (window as unknown as W).__rt.backend.git;
    const scan = git.scan.bind(git);
    let first = true;
    git.scan = async (paths) => {
      if (first) {
        first = false;
        return [];
      }
      return scan(paths);
    };
  });
  await page.getByTestId("view-git").click();
  await page.getByTestId("git-message").fill("Add auth");
  await page.getByTestId("git-commit").click();
  await expect(page.getByTestId("git-secret-finding")).toHaveCount(1);
  await expect(page.getByTestId("git-commit-subject")).toHaveText(["Initial collection"]);
});

test("external secrets: choose a provider and reference variables, no value field", async ({ page }) => {
  await openDemo(page);
  await page.getByRole("button", { name: "Environments" }).click();
  const manager = page.getByTestId("env-manager");
  await expect(manager).toBeVisible();
  const section = manager.getByTestId("external-secrets");
  await expect(section).toBeVisible();
  const selected = await manager.getByTestId("env-item").first().textContent();
  await manager.getByTestId("env-item").first().click();

  await section.getByTestId("external-provider").selectOption("one-password");
  await expect(section.getByTestId("external-help")).toContainText("op://");
  await section.getByTestId("external-add").click();
  const row = section.getByTestId("external-var").nth(0);
  await expect(row.getByTestId("external-reference")).toHaveAttribute("placeholder", "op://vault/item/field");
  await row.getByTestId("external-name").fill("apiToken");
  await row.getByTestId("external-name").press("Tab");
  await row.getByTestId("external-reference").fill("op://Dev/api/token");
  await row.getByTestId("external-reference").press("Tab");
  // References only: no value or password field in the section.
  await expect(section.locator('input[type="password"]')).toHaveCount(0);
  await expect(section.getByLabel(/value/i)).toHaveCount(0);

  const external = async () => {
    const envs = (await page.evaluate(() => (window as unknown as W).__rt.backend.environments())) as Environment[];
    return envs.find((e) => selected?.includes(e.name))?.external_secrets ?? null;
  };
  await expect.poll(external).toEqual({
    provider: "one-password",
    variables: [{ name: "apiToken", reference: "op://Dev/api/token", enabled: true }],
  });

  await row.getByTestId("external-enabled").uncheck();
  await expect.poll(async () => (await external())?.variables[0].enabled).toBe(false);
  await section.getByTestId("external-provider").selectOption("azure-key-vault");
  await expect.poll(async () => (await external())?.provider).toBe("azure-key-vault");
  await expect(row.getByTestId("external-reference")).toHaveAttribute("placeholder", "vault-name/secret-name");
  await row.getByTestId("external-remove").click();
  await expect.poll(async () => (await external())?.variables).toEqual([]);
  await section.getByTestId("external-provider").selectOption("");
  await expect.poll(external).toBeNull();
});
