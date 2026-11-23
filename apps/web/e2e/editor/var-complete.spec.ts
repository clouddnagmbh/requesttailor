// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Variable completion (wave 14): typing `{{` in any field that takes variables lists every
// variable the request sees, against the mock backend. Screenshots of the popup in light and
// dark go to `RT_SHOTS` when it is set.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Backend, Environment, RequestSpec } from "@requesttailor/ui";

type Rt = { backend: Backend & { files: Map<string, string> } };
declare global {
  interface Window {
    __rt: Rt;
  }
}

const LIST = "users/list-users.yml";
const CREATE = "users/create-user.yml";

async function openRequest(page: Page, name: string, before?: () => Promise<void>) {
  await page.goto("/?backend=mock");
  if (before) await before();
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
}

const cm = (field: Locator) => field.locator(".cm-content");
/** The completion popup (the variable list, not the editor's other tooltips). */
const popup = (page: Page) => page.locator(".cm-tooltip-autocomplete.rt-vc");
const option = (page: Page, name: string) =>
  popup(page).getByRole("option").filter({ has: page.locator(".cm-completionLabel", { hasText: new RegExp(`^${name}$`) }) });

async function clearAndType(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  await content.press("Backspace");
  await content.page().keyboard.type(text);
}

const backendSpec = (page: Page, path: string) =>
  page.evaluate((p) => window.__rt.backend.getRequest(p), path) as Promise<RequestSpec>;

async function expectSpec(page: Page, path: string, check: (s: RequestSpec) => unknown, expected: unknown) {
  await expect.poll(async () => check(await backendSpec(page, path))).toEqual(expected);
}

const shots = process.env.RT_SHOTS;

async function shoot(page: Page, name: string) {
  if (!shots) return;
  for (const scheme of ["light", "dark"] as const) {
    await page.emulateMedia({ colorScheme: scheme });
    await page.waitForTimeout(50);
    await page.screenshot({ path: `${shots}/${name}-${scheme}.png` });
  }
  await page.emulateMedia({ colorScheme: "light" });
}

test("URL: {{ opens every variable at once; Enter inserts {{baseUrl}}, highlighted as resolved", async ({ page }) => {
  await openRequest(page, "List users");
  const url = page.getByTestId("url-input");
  await clearAndType(url, "{{");
  await expect(popup(page)).toBeVisible();
  // Every layer: collection, environment (secret too), folder; then "Create variable…".
  for (const name of ["apiVersion", "baseUrl", "token", "userId"]) await expect(option(page, name)).toHaveCount(1);
  const rows = popup(page).getByRole("option");
  await expect(rows.last()).toContainText("Create variable");
  await expect(option(page, "baseUrl")).toContainText("http://127.0.0.1:47811");
  await expect(option(page, "baseUrl").locator(".rt-vc-badge")).toHaveText("EnvLocal");
  await expect(option(page, "userId").locator(".rt-vc-badge")).toHaveText("Folderusers");
  await expect(option(page, "apiVersion").locator(".rt-vc-badge")).toHaveText("Collection");
  await shoot(page, "url-all");

  // Typing filters, prefix matches first.
  await page.keyboard.type("ba");
  await expect(rows.first()).toContainText("baseUrl");
  await expect(option(page, "userId")).toHaveCount(0);
  await shoot(page, "url-filtered");
  await page.keyboard.press("Enter");
  await expect(popup(page)).toBeHidden();
  await expect(cm(url)).toHaveText("{{baseUrl}}");
  await expect(url.locator('.rt-var[data-var="baseUrl"]')).toHaveAttribute("data-var-state", "resolved");
  await expectSpec(page, LIST, (s) => s.url, "{{baseUrl}}");

  // An existing }} is completed, not doubled; Esc closes; Ctrl+Space reopens inside {{…
  await clearAndType(url, "{{}}/users");
  await cm(url).press("Home");
  await cm(url).press("ArrowRight");
  await cm(url).press("ArrowRight");
  await cm(url).press("Control+Space");
  await expect(popup(page)).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(popup(page)).toBeHidden();
  await cm(url).press("Control+Space");
  await expect(popup(page)).toBeVisible();
  await page.keyboard.type("api");
  await expect(popup(page).getByRole("option").first()).toContainText("apiVersion");
  await page.keyboard.press("Tab");
  await expect(cm(url)).toHaveText("{{apiVersion}}/users");
  await expect(url.locator('.rt-var[data-var="apiVersion"]')).toHaveAttribute("data-var-state", "resolved");
});

test("an overridden definition is marked", async ({ page }) => {
  await openRequest(page, "List users", () =>
    page.evaluate(async () => {
      const b = window.__rt.backend;
      await b.openCollection();
      await b.variables.define("", { name: "baseUrl", value: "http://collection.example", enabled: true, secret: false });
    }),
  );
  await clearAndType(page.getByTestId("url-input"), "{{base");
  const both = popup(page).getByRole("option");
  await expect(both).toHaveCount(2);
  await expect(both.nth(0)).toContainText("Local");
  await expect(both.nth(0)).not.toHaveClass(/rt-vc-overridden/);
  await expect(both.nth(1)).toHaveClass(/rt-vc-overridden/);
  await expect(both.nth(1).locator(".rt-vc-over")).toHaveAttribute("title", "Overridden by Environment Local");
  await expect(both.nth(1)).toContainText("http://collection.example");
});

test("header value: {{ completes there too", async ({ page }) => {
  await openRequest(page, "List users");
  await page.getByTestId("tab-headers").click();
  const table = page.getByTestId("headers-table");
  await table.getByTestId("kv-add").click();
  const row = table.getByTestId("kv-row").nth(1);
  await clearAndType(row.getByTestId("kv-name"), "Authorization");
  await clearAndType(row.getByTestId("kv-value"), "Bearer {{to");
  await expect(option(page, "token")).toBeVisible();
  await shoot(page, "header-narrow");
  await page.keyboard.press("Enter");
  await expect(cm(row.getByTestId("kv-value"))).toHaveText("Bearer {{token}}");
  await expectSpec(page, LIST, (s) => s.headers[1], { name: "Authorization", value: "Bearer {{token}}", enabled: true });
  // Header names complete as well.
  await clearAndType(row.getByTestId("kv-name"), "X-{{api");
  await expect(option(page, "apiVersion")).toBeVisible();
  await page.keyboard.press("Escape");
});

test("body: {{ inside JSON inserts the name without eating the text around it", async ({ page }) => {
  await openRequest(page, "Create user");
  await page.getByTestId("tab-body").click();
  const body = page.getByTestId("body-editor");
  const content = cm(body);
  await content.click();
  await content.press("ControlOrMeta+End");
  await content.press("ArrowUp");
  await content.press("End");
  await page.keyboard.type(',\n"id": {{us');
  await expect(option(page, "userId")).toBeVisible();
  await expect(option(page, "userId")).toContainText("42");
  await page.keyboard.press("Tab");
  await expectSpec(page, CREATE, (s) => (s.body.type === "raw" ? s.body.data : null), '{\n  "name": "Ada",\n  "id": {{userId}}\n}');
});

test("secrets are masked: •••• and the source of external ones, never the value", async ({ page }) => {
  await openRequest(page, "List users", () =>
    page.evaluate(async () => {
      const b = window.__rt.backend;
      await b.secrets.set("Local", "token", "s3cr3t-value");
      const envs: Environment[] = await b.environments();
      const local = envs.find((e) => e.name === "Local")!;
      await b.updateEnvironment({
        ...local,
        external_secrets: {
          provider: "one-password",
          variables: [{ name: "dbPassword", reference: "op://vault/db/password", enabled: true }],
        },
      });
    }),
  );
  await clearAndType(page.getByTestId("url-input"), "{{");
  await expect(option(page, "token").locator(".rt-vc-value")).toHaveText("••••");
  await expect(option(page, "dbPassword").locator(".rt-vc-value")).toHaveText("from 1Password");
  await expect(popup(page)).not.toContainText("s3cr3t");
  await shoot(page, "secrets");
  await page.keyboard.press("Escape");
});

test("create variable: in the collection (plain) and in the environment (secret)", async ({ page }) => {
  await openRequest(page, "List users");
  const url = page.getByTestId("url-input");
  await clearAndType(url, "{{baseUrl}}/{{tenant");
  const create = popup(page).getByRole("option").last();
  await expect(create).toContainText("Create variable “tenant”");
  await page.keyboard.press("ArrowUp");
  await expect(create).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Enter");
  const form = page.getByTestId("var-create-form");
  await expect(form).toBeVisible();
  await expect(form.getByTestId("var-create-name")).toHaveValue("tenant");
  await expect(form.getByTestId("var-create-value")).toBeFocused();
  await page.keyboard.type("acme");
  await form.getByTestId("var-create-in-collection").click();
  await expect(form.getByTestId("var-create-secret")).toBeDisabled();
  await shoot(page, "create-form");
  await form.getByTestId("var-create-submit").click();
  await expect(form).toBeHidden();
  await expect(cm(url)).toHaveText("{{baseUrl}}/{{tenant}}");
  await expect(url.locator('.rt-var[data-var="tenant"]')).toHaveAttribute("data-var-state", "resolved");
  await expect(page.getByTestId("url-preview")).toContainText("/acme");
  await expect.poll(() => page.evaluate(() => window.__rt.backend.dirty())).toContain("opencollection.yml");

  // A secret goes into the environment, its value only into the secret store.
  await clearAndType(url, "{{baseUrl}}?key={{apiKey");
  await popup(page).getByRole("option").last().click();
  await expect(form).toBeVisible();
  await form.getByTestId("var-create-in-environment").click();
  await form.getByTestId("var-create-secret").check();
  await expect(form.getByTestId("var-create-value")).toHaveAttribute("type", "password");
  await form.getByTestId("var-create-value").fill("k3y-s3cret");
  await form.getByTestId("var-create-value").press("Enter");
  await expect(form).toBeHidden();
  await expect(cm(url)).toHaveText("{{baseUrl}}?key={{apiKey}}");
  await expect(url.locator('.rt-var[data-var="apiKey"]')).toHaveAttribute("data-var-state", "secret");
  const env = await page.evaluate(async () => (await window.__rt.backend.environments()).find((e) => e.name === "Local")!);
  expect(env.variables.find((v) => v.name === "apiKey")).toEqual({ name: "apiKey", value: null, enabled: true, secret: true });
  expect(await page.evaluate(() => window.__rt.backend.secrets.get("Local", "apiKey"))).toBe("k3y-s3cret");
  await expect(page.getByTestId("request-editor")).not.toContainText("k3y-s3cret");

  // Esc cancels the form and returns to the field.
  await clearAndType(url, "{{other");
  await popup(page).getByRole("option").last().click();
  await expect(form).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(form).toBeHidden();
  await expect(cm(url)).toBeFocused();
  await expect(cm(url)).toHaveText("{{other");
});

test("environment values complete too", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByRole("button", { name: "Environments" }).click();
  const manager = page.getByTestId("env-manager");
  await manager.getByTestId("env-item").filter({ hasText: "Local" }).click();
  const value = manager.getByTestId("env-var").nth(0).getByTestId("var-value");
  await clearAndType(value, "{{api");
  await expect(option(page, "apiVersion")).toBeVisible();
  await shoot(page, "environment");
  await page.keyboard.press("Enter");
  await expect(cm(value)).toHaveText("{{apiVersion}}");
  await expect
    .poll(async () => (await page.evaluate(() => window.__rt.backend.environments()))[0].variables[0].value)
    .toBe("{{apiVersion}}");
});

test("no list in scripts: {{ is not a variable there", async ({ page }) => {
  await openRequest(page, "List users");
  await page.getByTestId("tab-scripts").click();
  const script = cm(page.getByTestId("script-before-request"));
  await script.click();
  await page.keyboard.type("const t = `{{");
  await page.waitForTimeout(300);
  await expect(popup(page)).toHaveCount(0);
});
