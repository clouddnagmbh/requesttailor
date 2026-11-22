// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 7, strand C, with the real core in the browser (rt-wasm + OPFS + rt-testserver):
// declarative assertions and a set-variable action run without JavaScript, and the value is
// chained into the next send through the session; "set as variable" from the JSON tree.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile } from "./env";

const LOGIN = `info:
  name: Login
  type: http
  seq: 1

http:
  method: GET
  url: "{{baseUrl}}/cookies/set?token=tok-abc&user=ada"

runtime:
  assertions:
    - expression: res.status
      operator: eq
      value: "200"
    - expression: res.body.token
      operator: startsWith
      value: tok-
    - expression: res.body.user
      operator: eq
      value: grace
  actions:
    - type: set-variable
      phase: after-response
      selector:
        expression: res.body.token
        method: jsonq
      variable:
        name: token
        scope: runtime
`;

const WHOAMI = `info:
  name: Who am I
  type: http
  seq: 2

http:
  method: GET
  url: "{{baseUrl}}/echo/whoami"
  headers:
    - name: X-Token
      value: "{{token}}"
`;

function collection(): Record<string, string> {
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Chaining\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n`,
    "environments/Other.yml": `name: Other\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n`,
    "login.yml": LOGIN,
    "whoami.yml": WHOAMI,
  };
}

async function setup(page: Page): Promise<void> {
  await fakePicker(page, "chaining");
  await page.goto("/");
  await fillFolder(page, collection(), "chaining");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Chaining");
  await page.getByTestId("env-switcher").selectOption("Local");
}

async function send(page: Page, name: string) {
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

/** The `X-Token` header as sent (the response's Headers tab). */
async function expectSentToken(page: Page, value: string) {
  await page.getByTestId("response-tab-headers").click();
  await expect(page.getByTestId("request-headers").locator("tr").filter({ hasText: "X-Token" })).toContainText(value);
}

test("assertions and an action against rt-testserver; the value is chained into the next send", async ({ page }) => {
  await setup(page);
  await send(page, "Who am I");
  await expectSentToken(page, "{{token}}");

  await send(page, "Login");
  await expect(page.getByTestId("response-tab-tests")).toHaveText("Tests 2/3");
  await page.getByTestId("response-tab-tests").click();
  const results = page.getByTestId("test-result");
  await expect(results.first()).toHaveAttribute("data-passed", "false");
  await expect(results.first()).toContainText("res.body.user eq grace");
  await expect(results.first().getByTestId("test-detail")).toHaveText("expected grace, got ada");
  await expect(page.getByTestId("session-vars-count")).toHaveText("1");

  await send(page, "Who am I");
  await expectSentToken(page, "tok-abc");
  // The editor's preview shows the session value as a runtime variable.
  await page.getByTestId("tab-headers").click();
  const resolved = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const p = await b.preview("whoami.yml", { environment: "Local", vars: [] });
    return p.variables.find((v) => v.name === "token");
  });
  expect(resolved).toMatchObject({ value: "tok-abc", source: { layer: "runtime" } });

  // Another environment has its own session; clearing empties this one.
  await page.getByTestId("env-switcher").selectOption("Other");
  await expect(page.getByTestId("session-vars-button")).toHaveCount(0);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("session-vars-button").click();
  await page.getByTestId("session-vars-clear").click();
  await page.getByTestId("session-vars-close").click();
  await send(page, "Who am I");
  await expectSentToken(page, "{{token}}");

  // Nothing was written: the request files are unchanged.
  expect(await readFolderFile(page, "login.yml", "chaining")).toBe(LOGIN);
  await expect(page.getByTestId("save")).toBeDisabled();
});

test("set as variable from the JSON tree feeds the next send", async ({ page }) => {
  await setup(page);
  await send(page, "Login");
  await page.getByTestId("session-vars-button").click();
  await page.getByTestId("session-vars-clear").click();
  await page.getByTestId("session-vars-close").click();

  await page.getByTestId("response-mode-tree").click();
  const user = page.getByTestId("response-tree").locator('li[data-path="$.user"]');
  await user.hover();
  await user.getByTestId("json-set-variable").click();
  const dialog = page.getByTestId("set-variable-dialog");
  await expect(dialog.getByTestId("set-variable-name")).toHaveValue("user");
  await dialog.getByTestId("set-variable-name").fill("token");
  await dialog.getByTestId("set-variable-save").click();
  await expect(page.getByTestId("session-vars-count")).toHaveText("1");

  await send(page, "Who am I");
  await expectSentToken(page, "ada");
});

test("edited assertions and actions are saved to the request file", async ({ page }) => {
  test.skip(true, "wave 7: needs strand B (format writer for runtime.assertions/actions)");
  await setup(page);
  await page.getByTestId("tree-request").filter({ hasText: "Who am I" }).click();
  await page.getByTestId("tab-assertions").click();
  await page.getByTestId("assertion-add").click();
  await page.getByTestId("action-add").click();
  await page.getByTestId("action-row").getByTestId("action-variable").fill("path");
  await page.getByTestId("action-row").getByTestId("action-expression").fill("res.body.path");
  await page.getByTestId("tab-scripts").click();
  await page.getByTestId("script-tests").locator(".cm-content").click();
  await page.keyboard.type('test("ok", () => {});');
  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  const text = await readFolderFile(page, "whoami.yml", "chaining");
  expect(text).toContain("expression: res.status");
  expect(text).toContain("name: path");
  expect(text).toContain('test("ok", () => {});');
});
