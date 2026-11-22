// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 7, strand C (scripts, assertions, chaining), against the mock
// backend: the Scripts and Assertions tabs of the request editor, the response's Tests and
// Console tabs, "set as variable" from the JSON tree, and session variables.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { MockBackend, RequestSpec } from "@requesttailor/ui";

declare global {
  interface Window {
    __rt: { backend: MockBackend };
  }
}

const HEALTH = "health.yml";
const LIST = "users/list-users.yml";

/** Opens the mock collection, applies `change` to `path` through the backend (unsaved), then
 *  opens the request. */
async function open(page: Page, name: string, setup?: { path: string; patch: Partial<RequestSpec> }[]) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
  for (const s of setup ?? []) {
    await page.evaluate(async ({ path, patch }) => {
      const b = window.__rt.backend;
      await b.updateRequest(path, { ...(await b.getRequest(path)), ...patch });
    }, s);
  }
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
}

const backendSpec = (page: Page, path: string) =>
  page.evaluate((p) => window.__rt.backend.getRequest(p), path) as Promise<RequestSpec>;

async function expectSpec(page: Page, path: string, check: (s: RequestSpec) => unknown, expected: unknown) {
  await expect.poll(async () => check(await backendSpec(page, path))).toEqual(expected);
}

const cm = (field: Locator) => field.locator(".cm-content");

async function send(page: Page) {
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

const LOGIN = {
  path: HEALTH,
  patch: {
    url: "{{baseUrl}}/login",
    assertions: [
      { expression: "res.status", operator: "eq", value: "200", description: null, enabled: true },
      { expression: "res.body.token", operator: "eq", value: "wrong", description: null, enabled: true },
    ],
    actions: [
      {
        phase: "after-response" as const,
        expression: "res.body.token",
        method: "jsonq",
        variable: "sessionToken",
        scope: "runtime" as const,
        enabled: true,
      },
    ],
  },
};

test.describe("request editor", () => {
  test("Scripts tab: three JavaScript editors bound to the request's scripts", async ({ page }) => {
    await open(page, "Health");
    await expect(page.getByTestId("tab-scripts")).toHaveText("Scripts");
    await page.getByTestId("tab-scripts").click();
    for (const phase of ["before-request", "after-response", "tests"]) {
      await expect(page.getByTestId(`script-${phase}`)).toBeVisible();
    }
    // The JavaScript language arrives with the tab.
    await expect(page.getByTestId("script-tests")).toHaveAttribute("data-language", "ready");

    await cm(page.getByTestId("script-tests")).click();
    await page.keyboard.type('test("ok", () => expect(rt.response.status).to.equal(200));');
    await cm(page.getByTestId("script-before-request")).click();
    await page.keyboard.type('rt.vars.set("a", "1");');
    await expectSpec(page, HEALTH, (s) => s.scripts, [
      { phase: "tests", code: 'test("ok", () => expect(rt.response.status).to.equal(200));' },
      { phase: "before-request", code: 'rt.vars.set("a", "1");' },
    ]);
    await expect(page.getByTestId("tab-scripts")).toContainText("2");
    await expect(page.getByTestId("save")).toBeEnabled();
    // The script keywords are highlighted by the JavaScript language.
    await expect(page.getByTestId("script-tests").locator(".cm-line span").first()).toBeVisible();

    // Emptying a script removes it.
    await cm(page.getByTestId("script-before-request")).click();
    await page.keyboard.press("ControlOrMeta+a");
    await page.keyboard.press("Backspace");
    await expectSpec(page, HEALTH, (s) => s.scripts.map((x) => x.phase), ["tests"]);

    await page.getByTestId("save").click();
    await expect(page.getByTestId("save")).toBeDisabled();
    const file = await page.evaluate((p) => window.__rt.backend.files.get(p), HEALTH);
    expect(file).toContain("rt.response.status");
  });

  test("Assertions tab: table editor for assertions and actions, with badges", async ({ page }) => {
    await open(page, "Health");
    await page.getByTestId("tab-assertions").click();
    await page.getByTestId("assertion-add").click();
    await expect(page.getByTestId("assertion-row")).toHaveCount(1);
    await expectSpec(page, HEALTH, (s) => s.assertions, [
      { expression: "res.status", operator: "eq", value: "200", description: null, enabled: true },
    ]);
    await expect(page.getByTestId("tab-assertions")).toContainText("1");

    await page.getByTestId("assertion-add").click();
    const second = page.getByTestId("assertion-row").nth(1);
    await second.getByTestId("assertion-expression").fill("res.body.items");
    await second.getByTestId("assertion-operator").selectOption("isArray");
    await expect(second.getByTestId("assertion-value")).toBeDisabled();
    await second.getByTestId("assertion-enabled").uncheck();
    await expectSpec(
      page,
      HEALTH,
      (s) => s.assertions[1],
      { expression: "res.body.items", operator: "isArray", value: "200", description: null, enabled: false },
    );
    await expect(page.getByTestId("tab-assertions")).toContainText("1");

    await page.getByTestId("action-add").click();
    const action = page.getByTestId("action-row");
    await action.getByTestId("action-variable").fill("userId");
    await action.getByTestId("action-expression").fill("res.body.user.id");
    await action.getByTestId("action-scope").selectOption("environment");
    await expectSpec(page, HEALTH, (s) => s.actions, [
      { phase: "after-response", expression: "res.body.user.id", method: "jsonq", variable: "userId", scope: "environment", enabled: true },
    ]);
    await expect(page.getByTestId("tab-assertions")).toContainText("2");

    await second.getByTestId("assertion-remove").click();
    await expectSpec(page, HEALTH, (s) => s.assertions.length, 1);
    await expect(page.getByTestId("save")).toBeEnabled();
  });
});

test.describe("response", () => {
  test("Tests tab: assertion results, failed first, counts in the label", async ({ page }) => {
    await open(page, "Health", [LOGIN]);
    await send(page);
    const tab = page.getByTestId("response-tab-tests");
    await expect(tab).toHaveText("Tests 1/2");
    await tab.click();
    const results = page.getByTestId("test-result");
    await expect(results).toHaveCount(2);
    await expect(results.first()).toHaveAttribute("data-passed", "false");
    await expect(results.first()).toContainText("res.body.token eq wrong");
    await expect(results.first().getByTestId("test-detail")).toHaveText("expected wrong, got mock-token-123");
    await expect(results.nth(1)).toHaveAttribute("data-passed", "true");
    await expect(page.getByTestId("tests-summary")).toHaveText("1 of 2 passed, 1 failed");
  });

  test("Tests and Console tabs show script results, logs and notes", async ({ page }) => {
    await open(page, "Health");
    await page.evaluate(() => {
      window.__rt.backend.scriptOutcome = () => ({
        tests: [
          { name: "status is 200", passed: true, origin: "request" },
          { name: "has a user", passed: false, error: "expected undefined to exist", origin: "collection" },
        ],
        logs: [
          { level: "log", text: "hello from the script", origin: "request" },
          { level: "warn", text: "careful", origin: "folder:users" },
        ],
        notes: [{ source: "script", level: "warning", text: "Scripts were not run: this collection is not trusted yet." }],
      });
    });
    await send(page);
    await expect(page.getByTestId("response-tab-tests")).toHaveText("Tests 1/2");
    await expect(page.getByTestId("response-tab-console")).toHaveText("Console (2)");
    await page.getByTestId("response-tab-tests").click();
    await expect(page.getByTestId("test-result").first()).toContainText("has a user");
    await expect(page.getByTestId("test-result").first()).toContainText("expected undefined to exist");
    await expect(page.getByTestId("tests-note")).toContainText("not trusted");
    await page.getByTestId("response-tab-console").click();
    const lines = page.getByTestId("console-line");
    await expect(lines).toHaveCount(2);
    await expect(lines.first()).toContainText("hello from the script");
    await expect(lines.nth(1)).toHaveAttribute("data-level", "warn");
    await expect(lines.nth(1)).toContainText("folder:users");
  });

  test("a pre-request script error shows clearly, with its console output", async ({ page }) => {
    await open(page, "Health");
    await page.evaluate(() => {
      window.__rt.backend.scriptOutcome = () => ({
        error: { kind: "script", message: "ReferenceError: tokn is not defined (request:2)" },
        logs: [{ level: "log", text: "about to sign", origin: "request" }],
      });
    });
    await page.getByTestId("send").click();
    const error = page.getByTestId("response-error");
    await expect(error).toHaveText("A pre-request script failed: ReferenceError: tokn is not defined (request:2)");
    await expect(error).toHaveAttribute("data-kind", "script");
    await expect(page.getByTestId("console-line")).toHaveText(/about to sign/);
  });
});

test.describe("chaining", () => {
  test("an action's value goes into the next send and shows as a session variable", async ({ page }) => {
    await open(page, "Health", [
      LOGIN,
      { path: LIST, patch: { headers: [{ name: "X-Token", value: "{{sessionToken}}", enabled: true }] } },
    ]);
    await expect(page.getByTestId("session-vars-button")).toHaveCount(0);
    await send(page);
    await expect(page.getByTestId("session-vars-count")).toHaveText("1");

    await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
    await send(page);
    const sent = await page.evaluate(async () => {
      const b = window.__rt.backend;
      return (await b.send("users/list-users.yml", { environment: "Local", vars: [] })).report.request?.headers;
    });
    expect(sent).toContainEqual(["X-Token", "mock-token-123"]);

    // The session list, per environment, with remove and clear all.
    await page.getByTestId("session-vars-button").click();
    const dialog = page.getByTestId("session-vars-dialog");
    await expect(dialog.getByTestId("session-var-name")).toHaveText("sessionToken");
    await expect(dialog.getByTestId("session-var-value")).toHaveText("mock-token-123");
    await dialog.getByTestId("session-vars-clear").click();
    await expect(dialog.getByTestId("session-vars-empty")).toBeVisible();
    await dialog.getByTestId("session-vars-close").click();
    await expect(page.getByTestId("session-vars-button")).toHaveCount(0);
  });

  test("session variables belong to their environment", async ({ page }) => {
    await open(page, "Health", [LOGIN]);
    await send(page);
    await expect(page.getByTestId("session-vars-count")).toHaveText("1");
    await page.getByTestId("env-switcher").selectOption("PRD");
    await expect(page.getByTestId("session-vars-button")).toHaveCount(0);
    await page.getByTestId("env-switcher").selectOption("Local");
    await expect(page.getByTestId("session-vars-count")).toHaveText("1");
  });

  test("set as variable from the JSON tree, also as an action of the request", async ({ page }) => {
    await open(page, "Health", [{ path: HEALTH, patch: { url: "{{baseUrl}}/login" } }]);
    await send(page);
    await page.getByTestId("response-mode-tree").click();
    const tree = page.getByTestId("response-tree");
    const token = tree.locator('li[data-path="$.token"]');
    await token.hover();
    await token.getByTestId("json-set-variable").click();

    const dialog = page.getByTestId("set-variable-dialog");
    await expect(dialog.getByTestId("set-variable-name")).toHaveValue("token");
    await expect(dialog.getByTestId("set-variable-value")).toHaveText("mock-token-123");
    await expect(dialog.getByTestId("set-variable-scope")).toContainText("Local");
    await dialog.getByTestId("set-variable-name").fill("authToken");
    await dialog.getByTestId("set-variable-action").check();
    await dialog.getByTestId("set-variable-save").click();
    await expect(dialog).toHaveCount(0);

    await expect(page.getByTestId("session-vars-count")).toHaveText("1");
    await expectSpec(page, HEALTH, (s) => s.actions, [
      { phase: "after-response", expression: "res.body.token", method: "jsonq", variable: "authToken", scope: "runtime", enabled: true },
    ]);
    await expect(page.getByTestId("save")).toBeEnabled();
    await page.getByTestId("tab-assertions").click();
    await expect(page.getByTestId("action-row").getByTestId("action-variable")).toHaveValue("authToken");

    // The value is a runtime variable in the editor's preview.
    await page.getByTestId("tab-headers").click();
    await page.evaluate(async () => {
      const b = window.__rt.backend;
      const s = await b.getRequest("users/list-users.yml");
      await b.updateRequest("users/list-users.yml", { ...s, url: "{{baseUrl}}/users?t={{authToken}}" });
    });
    await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
    await expect(page.getByTestId("url-preview")).toContainText("t=mock-token-123");
  });

  test("set as variable works from the keyboard", async ({ page }) => {
    await open(page, "Health", [{ path: HEALTH, patch: { url: "{{baseUrl}}/login" } }]);
    await send(page);
    await page.getByTestId("response-mode-tree").click();
    const tree = page.getByTestId("response-tree");
    // Expand `user` from the keyboard, then reach the button of `user.id`.
    const toggle = tree.locator('li[data-path="$.user"] > button[aria-expanded]');
    await toggle.focus();
    await page.keyboard.press("Enter");
    await expect(toggle).toHaveAttribute("aria-expanded", "true");
    const pick = tree.locator('li[data-path="$.user.id"]').getByTestId("json-set-variable");
    await expect(pick).toHaveCSS("opacity", "0");
    await pick.focus();
    await expect(pick).toHaveCSS("opacity", "1");
    await page.keyboard.press("Enter");
    const dialog = page.getByTestId("set-variable-dialog");
    await expect(dialog.getByTestId("set-variable-name")).toBeFocused();
    await expect(dialog.getByTestId("set-variable-name")).toHaveValue("id");
    await expect(dialog.getByTestId("set-variable-value")).toHaveText("7");
    await page.keyboard.press("Enter");
    await expect(dialog).toHaveCount(0);
    const vars = await page.evaluate(() => window.__rt.backend.scripts.sessionVars("Local"));
    expect(vars).toEqual([{ name: "id", value: "7", scope: "runtime" }]);
    // Escape closes without setting.
    await pick.focus();
    await page.keyboard.press("Enter");
    await page.keyboard.press("Escape");
    await expect(dialog).toHaveCount(0);
  });
});
