// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand D acceptance: environment switcher, environment manager, secrets never in files.

import { expect, test, type Locator } from "@playwright/test";

import { environments, files, openDemo, openManager, secret, sendAndReport, type TestWindow } from "./helpers";

const SECRET = "s3cr3t-Staging-Token-42";

/** Plain values are CodeFields (wave 14: `{{` completion): type into the editor, then leave. */
async function typeValue(row: Locator, text: string) {
  const content = row.getByTestId("var-value").locator(".cm-content");
  await content.click();
  await content.page().keyboard.type(text);
  await content.press("Tab");
}

test("switching the environment changes the next send", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  const switcher = page.getByTestId("env-switcher");

  await switcher.selectOption("PRD");
  let report = await sendAndReport(page);
  expect(report.environment).toBe("PRD");
  expect(report.request?.url).toBe("https://api.example.com/users?page=1");

  await switcher.selectOption("Local");
  report = await sendAndReport(page);
  expect(report.environment).toBe("Local");
  expect(report.request?.url).toBe("http://127.0.0.1:47811/users?page=1");

  await switcher.selectOption("");
  report = await sendAndReport(page);
  expect(report.environment).toBeNull();
  expect(report.unresolved).toContain("baseUrl");
});

test("the active environment is marked and production names get a warning", async ({ page }) => {
  await openDemo(page);
  const switcher = page.getByTestId("env-switcher");
  await expect(switcher).toHaveValue("Local");
  await expect(switcher).toHaveAttribute("data-prod", "false");
  await switcher.selectOption("PRD");
  await expect(switcher).toHaveAttribute("data-prod", "true");
  await expect(page.getByTestId("env-active")).toHaveText("PRD");
  // The warning is a different colour from the normal state.
  const warn = await switcher.evaluate((e) => getComputedStyle(e).borderColor);
  await switcher.selectOption("Local");
  const normal = await switcher.evaluate((e) => getComputedStyle(e).borderColor);
  expect(warn).not.toBe(normal);
});

test("create an environment with a secret: saved files never hold the value, send uses it", async ({ page }) => {
  await openDemo(page);
  const manager = await openManager(page);

  await manager.getByTestId("env-new").click();
  await manager.getByTestId("env-new-name").fill("Staging");
  await manager.getByTestId("env-create").click();
  await expect(manager.getByTestId("env-item").filter({ hasText: "Staging" })).toHaveAttribute(
    "aria-current",
    "true",
  );

  await manager.getByTestId("var-add").click();
  let row = manager.getByTestId("env-var").nth(0);
  await row.getByTestId("var-name").fill("baseUrl");
  await row.getByTestId("var-name").press("Tab");
  await typeValue(row, "http://127.0.0.1:47811");

  await manager.getByTestId("var-add").click();
  row = manager.getByTestId("env-var").nth(1);
  await row.getByTestId("var-name").fill("token");
  await row.getByTestId("var-name").press("Tab");
  await row.getByTestId("var-secret").check();
  await expect(row.getByTestId("var-value")).toHaveAttribute("type", "password");
  await row.getByTestId("var-value").fill(SECRET);
  await row.getByTestId("var-value").press("Tab");
  await expect.poll(() => secret(page, "Staging", "token")).toBe(SECRET);

  await manager.getByTestId("env-save").click();
  await expect.poll(async () => (await files(page)).map(([p]) => p)).toContain("environments/Staging.yml");
  const written = await files(page);
  const staging = written.find(([p]) => p === "environments/Staging.yml")![1];
  expect(staging).toContain("token");
  expect(staging).toContain("baseUrl");
  for (const [path, text] of written) expect(text, path).not.toContain(SECRET);
  const env = (await environments(page)).find((e) => e.name === "Staging")!;
  expect(env.variables.find((v) => v.name === "token")).toEqual({
    name: "token",
    value: null,
    enabled: true,
    secret: true,
  });
  // The page never shows the value unless revealed.
  await expect(row.getByTestId("var-value")).not.toHaveValue(SECRET);
  await row.getByTestId("var-reveal").click();
  await expect(row.getByTestId("var-value")).toHaveValue(SECRET);
  await expect(row.getByTestId("var-value")).toHaveAttribute("type", "text");
  await row.getByTestId("var-reveal").click();
  await expect(row.getByTestId("var-value")).toHaveAttribute("type", "password");
  await expect(row.getByTestId("var-value")).not.toHaveValue(SECRET);

  await manager.getByTestId("env-close").click();
  await expect(manager).toBeHidden();

  await page.getByTestId("env-switcher").selectOption("Staging");
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  const report = await sendAndReport(page);
  expect(report.environment).toBe("Staging");
  expect(report.request?.url).toBe("http://127.0.0.1:47811/users");
  expect(report.unresolved).toEqual([]);
  expect(report.request?.headers).toContainEqual(["Authorization", "Bearer ***"]);
  expect(report.variables).toContainEqual({
    name: "token",
    source: { layer: "environment", name: "Staging" },
    secret: true,
  });
  expect(JSON.stringify(report)).not.toContain(SECRET);
});

test("delete an environment removes it and its secret values", async ({ page }) => {
  await openDemo(page);
  await page.evaluate(() =>
    (window as unknown as TestWindow).__rt.backend.secrets.set("PRD", "token", "prd-token"),
  );
  await page.getByTestId("env-switcher").selectOption("PRD");
  const manager = await openManager(page);
  await manager.getByTestId("env-item").filter({ hasText: "PRD" }).click();
  await manager.getByTestId("env-delete").click();
  // Deleting asks inline first.
  await manager.getByTestId("env-delete-cancel").click();
  expect((await environments(page)).map((e) => e.name)).toContain("PRD");
  await manager.getByTestId("env-delete").click();
  await manager.getByTestId("env-delete-confirm").click();

  await expect(manager.getByTestId("env-item").filter({ hasText: "PRD" })).toHaveCount(0);
  expect((await environments(page)).map((e) => e.name)).toEqual(["Local"]);
  expect(await secret(page, "PRD", "token")).toBeNull();
  await expect(page.getByTestId("env-switcher").locator("option")).toHaveText(["No environment", "Local"]);
  // The deleted environment was active: the switcher falls back to another one.
  await expect(page.getByTestId("env-switcher")).toHaveValue("Local");

  await manager.getByTestId("env-save").click();
  await expect.poll(async () => new Map(await files(page)).get("environments/PRD.yml")).toBe("");
});

test("plain to secret moves the value; secret to plain needs confirmation", async ({ page }) => {
  await openDemo(page);
  const manager = await openManager(page);
  await manager.getByTestId("env-item").filter({ hasText: "Local" }).click();

  await manager.getByTestId("var-add").click();
  const row = manager.getByTestId("env-var").nth(2);
  await row.getByTestId("var-name").fill("apiKey");
  await row.getByTestId("var-name").press("Tab");
  await typeValue(row, "plain-key-123");
  await expect
    .poll(async () => (await environments(page))[0].variables.find((v) => v.name === "apiKey")?.value)
    .toBe("plain-key-123");

  await row.getByTestId("var-secret").check();
  await expect.poll(() => secret(page, "Local", "apiKey")).toBe("plain-key-123");
  let v = (await environments(page))[0].variables.find((x) => x.name === "apiKey")!;
  expect(v).toMatchObject({ value: null, secret: true });

  // Back to plain: the value would land in a file, so it asks first.
  await row.getByTestId("var-secret").uncheck();
  await expect(row.getByTestId("var-plain-confirm")).toBeVisible();
  await row.getByTestId("var-plain-cancel").click();
  await expect(row.getByTestId("var-secret")).toBeChecked();
  expect(await secret(page, "Local", "apiKey")).toBe("plain-key-123");

  await row.getByTestId("var-secret").uncheck();
  await row.getByTestId("var-plain-confirm").click();
  await expect(row.getByTestId("var-secret")).not.toBeChecked();
  await expect.poll(() => secret(page, "Local", "apiKey")).toBeNull();
  v = (await environments(page))[0].variables.find((x) => x.name === "apiKey")!;
  expect(v).toMatchObject({ value: "plain-key-123", secret: false });
  await expect(row.getByTestId("var-value").locator(".cm-content")).toHaveText("plain-key-123");
});

test("rename and duplicate keep secret values", async ({ page }) => {
  await openDemo(page);
  const manager = await openManager(page);
  await manager.getByTestId("env-item").filter({ hasText: "Local" }).click();
  const token = manager.getByTestId("env-var").nth(1);
  await expect(token.getByTestId("var-name")).toHaveValue("token");
  await token.getByTestId("var-value").fill("local-token");
  await token.getByTestId("var-value").press("Tab");
  await expect.poll(() => secret(page, "Local", "token")).toBe("local-token");

  await manager.getByTestId("env-name").fill("Dev");
  await manager.getByTestId("env-rename").click();
  await expect(manager.getByTestId("env-item").filter({ hasText: "Dev" })).toHaveAttribute("aria-current", "true");
  expect((await environments(page)).map((e) => e.name).sort()).toEqual(["Dev", "PRD"]);
  expect(await secret(page, "Dev", "token")).toBe("local-token");
  expect(await secret(page, "Local", "token")).toBeNull();
  // Local was active; the switcher follows the rename.
  await expect(page.getByTestId("env-switcher")).toHaveValue("Dev");

  // Renaming onto an existing name is refused inline.
  await manager.getByTestId("env-name").fill("PRD");
  await manager.getByTestId("env-rename").click();
  await expect(manager.getByTestId("env-error")).toContainText("PRD");
  expect((await environments(page)).map((e) => e.name).sort()).toEqual(["Dev", "PRD"]);

  await manager.getByTestId("env-duplicate").click();
  await expect(manager.getByTestId("env-item").filter({ hasText: "Dev copy" })).toHaveAttribute(
    "aria-current",
    "true",
  );
  expect(await secret(page, "Dev copy", "token")).toBe("local-token");
  expect(await secret(page, "Dev", "token")).toBe("local-token");
  const copy = (await environments(page)).find((e) => e.name === "Dev copy")!;
  expect(copy.variables.map((x) => x.name)).toEqual(["baseUrl", "token"]);

  await manager.getByTestId("env-save").click();
  for (const [path, text] of await files(page)) expect(text, path).not.toContain("local-token");
});

test("the manager is a dialog without browser popups and closes with Escape", async ({ page }) => {
  page.on("dialog", (d) => {
    throw new Error(`unexpected browser dialog: ${d.type()} ${d.message()}`);
  });
  await openDemo(page);
  const manager = await openManager(page);
  await manager.getByTestId("env-item").filter({ hasText: "PRD" }).click();
  await manager.getByTestId("env-delete").click();
  await manager.getByTestId("env-delete-cancel").click();
  await page.keyboard.press("Escape");
  await expect(manager).toBeHidden();
});
