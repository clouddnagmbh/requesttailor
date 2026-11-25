// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Documentation screenshots, set "Free I": getting started, sending, environments, auth,
// scripts, import and collections. Runs only with RT_DOC_SHOTS=<dir> (writes <id>.png there);
// skipped otherwise.
//   RT_DOC_SHOTS=/tmp/shots RT_WEB_PORT=48900 npx playwright test e2e/docshots/free-1.spec.ts
// Mock backend only; every host is example.com or loopback, every name invented.

import { expect, test, type Locator, type Page } from "@playwright/test";

const DIR = process.env.RT_DOC_SHOTS;
const MOD = "ControlOrMeta";

test.skip(!DIR, "set RT_DOC_SHOTS=<dir> to write the documentation screenshots");
test.setTimeout(120_000);
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, colorScheme: "light" });

type Clip = { x: number; y: number; width: number; height: number };

async function shot(page: Page, id: string, clip?: Clip) {
  await page.mouse.move(1430, 890);
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${DIR}/${id}.png`, ...(clip ? { clip } : {}) });
}

async function open(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function command(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill(label);
  await page.getByTestId("palette-option").filter({ hasText: label }).first().click();
}

const request = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name }).click();

async function send(page: Page) {
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

/** Gives the request half of the window the most room (keyboard End on the splitter). */
async function tallEditor(page: Page) {
  await page.getByTestId("split-resizer").focus();
  await page.keyboard.press("End");
  await page.waitForTimeout(200);
}

const cm = (field: Locator) => field.locator(".cm-content");

async function setText(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  if (text === "") await content.press("Backspace");
  else await content.page().keyboard.type(text);
}

// ---------------------------------------------------------------- sending

test("senden-request", async ({ page }) => {
  await open(page);
  await request(page, "Create user");
  await page.getByTestId("tab-body").click();
  await shot(page, "senden-request", { x: 261, y: 40, width: 1179, height: 330 });
});

test("senden-antwort", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await send(page);
  await shot(page, "senden-antwort");
});

test("senden-produktion", async ({ page }) => {
  await open(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("confirm-send")).toBeVisible();
  await shot(page, "senden-produktion");
});

test("senden-snapshot", async ({ page }) => {
  await open(page);
  await request(page, "Health");
  await send(page);
  await page.getByTestId("snapshot-freeze").click();
  await expect(page.getByTestId("snapshot-badge")).toHaveAttribute("data-state", "matches");
  await page.getByTestId("url-input").locator(".cm-content").click();
  await page.keyboard.press("End");
  await page.keyboard.type("?v=2");
  await send(page);
  await page.getByTestId("snapshot-badge").click();
  await expect(page.getByTestId("snapshot-dialog").getByTestId("snapshot-difference").first()).toBeVisible();
  await shot(page, "senden-snapshot");
});

test("senden-teilen", async ({ page }) => {
  await open(page);
  await request(page, "Create user");
  await command(page, "Share: link and QR code");
  const dialog = page.getByTestId("share-dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByTestId("share-toggle-preview").click();
  await expect(page.getByTestId("share-preview")).toBeVisible();
  await shot(page, "senden-teilen");
});

test("senden-timing", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await send(page);
  await page.getByTestId("response-tab-timing").click();
  await shot(page, "senden-timing", { x: 261, y: 500, width: 1179, height: 270 });
});

// ---------------------------------------------------------------- getting started

test("start-palette", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.keyboard.press(`${MOD}+k`);
  await expect(page.getByTestId("palette")).toBeVisible();
  await shot(page, "start-palette");
});

test("start-erster-request", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await send(page);
  await shot(page, "start-erster-request");
});

// ---------------------------------------------------------------- environments

test("umgebungen-herkunft", async ({ page }) => {
  await open(page);
  await request(page, "Create user");
  const baseUrl = page.getByTestId("url-input").locator('.rt-var[data-var="baseUrl"]');
  await expect(baseUrl).toHaveAttribute("data-var-state", "resolved");
  await baseUrl.hover();
  await expect(page.getByTestId("var-tooltip").first()).toContainText("environment Local");
  await page.waitForTimeout(300);
  await page.screenshot({ path: `${DIR}/umgebungen-herkunft.png`, clip: { x: 261, y: 30, width: 760, height: 190 } });
});

test("umgebungen-liste", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.evaluate(() => (window as any).__rt.backend.secrets.set("Local", "token", "demo-token-value"));
  await page.getByRole("button", { name: "Environments" }).click();
  await expect(page.getByTestId("env-manager")).toBeVisible();
  await shot(page, "umgebungen-liste");
});

test("umgebungen-schalter", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("env-switcher").selectOption("PRD");
  await send(page);
  await shot(page, "umgebungen-schalter");
});

// ---------------------------------------------------------------- auth

test("auth-oauth", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("tab-auth").click();
  await page.getByTestId("auth-type").selectOption("oauth2");
  await page.getByTestId("oauth2-flow").selectOption({ label: "Authorization code" });
  await setText(page.getByTestId("oauth2-authorization-url"), "https://idp.example.com/oauth/authorize");
  await setText(page.getByTestId("oauth2-callback-url"), "http://127.0.0.1:8765/callback");
  await setText(page.getByTestId("oauth2-token-url"), "https://idp.example.com/oauth/token");
  await setText(page.getByTestId("oauth2-client-id"), "demo-web-app");
  await setText(page.getByTestId("oauth2-scope"), "openid profile orders.read");
  await tallEditor(page);
  await page.getByTestId("oauth2-flow").evaluate((el) => {
    for (let p = el.parentElement; p; p = p.parentElement) p.scrollTop = 0;
  });
  await shot(page, "auth-oauth", { x: 261, y: 40, width: 1179, height: 712 });
});

test("auth-token", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("tab-auth").click();
  await page.getByTestId("auth-type").selectOption("oauth2");
  await page.getByTestId("oauth2-get-token").click();
  await expect(page.getByTestId("oauth2-token-status")).toContainText("Token received");
  await page.getByTestId("oauth2-tokens").click();
  await expect(page.getByTestId("tokens-dialog")).toBeVisible();
  await shot(page, "auth-token");
});

test("auth-arten", async ({ page }) => {
  await open(page);
  await request(page, "Create user");
  await page.getByTestId("tab-auth").click();
  await page.getByTestId("auth-type").selectOption("bearer");
  await shot(page, "auth-arten");
});

test("auth-aws", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("tab-auth").click();
  await page.getByTestId("auth-type").selectOption("awsv4");
  await setText(page.getByTestId("awsv4-service"), "execute-api");
  await setText(page.getByTestId("awsv4-region"), "eu-central-1");
  await shot(page, "auth-aws", { x: 261, y: 40, width: 1179, height: 400 });
});

// ---------------------------------------------------------------- scripts

test("skripte-editor", async ({ page }) => {
  await open(page);
  await request(page, "Health");
  await page.getByTestId("tab-scripts").click();
  await expect(page.getByTestId("script-tests")).toHaveAttribute("data-language", "ready");
  await cm(page.getByTestId("script-before-request")).click();
  await page.keyboard.type('rt.vars.set("requestedAt", Date.now());', { delay: 0 });
  await cm(page.getByTestId("script-tests")).click();
  await page.keyboard.type('test("status is 200", () => expect(rt.response.status).to.equal(200));', { delay: 0 });
  await tallEditor(page);
  await shot(page, "skripte-editor", { x: 261, y: 40, width: 1179, height: 560 });
});

test("skripte-assertions", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("tab-assertions").click();
  await page.getByTestId("assertion-add").click();
  await page.getByTestId("assertion-add").click();
  const second = page.getByTestId("assertion-row").nth(1);
  await second.getByTestId("assertion-expression").fill("res.body.items");
  await second.getByTestId("assertion-operator").selectOption("isArray");
  await shot(page, "skripte-assertions", { x: 261, y: 40, width: 1179, height: 330 });
});

test("skripte-tests", async ({ page }) => {
  await open(page);
  await request(page, "Health");
  await page.evaluate(() => {
    (window as any).__rt.backend.scriptOutcome = () => ({
      tests: [
        { name: "status is 200", passed: true, origin: "request" },
        { name: "has a user id", passed: false, error: "expected undefined to exist", origin: "request" },
      ],
      logs: [],
    });
  });
  await send(page);
  await page.getByTestId("response-tab-tests").click();
  await shot(page, "skripte-tests", { x: 261, y: 500, width: 1179, height: 170 });
});

test("skripte-runner", async ({ page }) => {
  await open(page);
  const row = page.getByTestId("tree-folder").filter({ hasText: "users" });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await expect(page.getByTestId("runner-panel")).toBeVisible();
  await page.getByTestId("runner-iterations").fill("3");
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-start")).toHaveText("Run again");
  await page.getByTestId("runner-step").first().click();
  await shot(page, "skripte-runner");
});

// ---------------------------------------------------------------- import

const POSTMAN = JSON.stringify({
  info: { name: "Shop", schema: "https://schema.getpostman.com/json/collection/v2.1.0/collection.json" },
  item: [],
});

test("import-dialog", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("import").click();
  await expect(page.getByTestId("import-dialog")).toBeVisible();
  await shot(page, "import-dialog");
});

test("import-bericht", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("import").click();
  const transfer = await page.evaluateHandle((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "shop.postman_collection.json", { type: "application/json" }));
    dt.items.add(new File(["{}"], "prod.postman_environment.json", { type: "application/json" }));
    return dt;
  }, POSTMAN);
  await page.getByTestId("import-drop").dispatchEvent("drop", { dataTransfer: transfer });
  await expect(page.getByTestId("import-plan")).toContainText("Loss report");
  await page.getByTestId("import-group").nth(0).locator("summary").click();
  await page.getByTestId("import-group").nth(3).locator("summary").click();
  await shot(page, "import-bericht");
});

test("import-ordner", async ({ page }) => {
  await open(page);
  await request(page, "List users");
  await page.getByTestId("import").click();
  const transfer = await page.evaluateHandle((text) => {
    const dt = new DataTransfer();
    dt.items.add(new File([text], "shop.postman_collection.json", { type: "application/json" }));
    return dt;
  }, POSTMAN);
  await page.getByTestId("import-drop").dispatchEvent("drop", { dataTransfer: transfer });
  await expect(page.getByTestId("import-plan")).toContainText("Loss report");
  await page.getByTestId("import-target").selectOption("folder");
  await page.getByTestId("import-write").click();
  await expect(page.getByTestId("import-dialog")).toBeHidden();
  await expect(page.getByTestId("tree-folder").filter({ hasText: "shop" })).toBeVisible();
  await shot(page, "import-ordner");
});

// ---------------------------------------------------------------- collections

test("collections-seitenleiste", async ({ page }) => {
  await open(page);
  await page.getByTestId("tree-collection").filter({ hasText: "Billing API" }).click();
  await request(page, "Billing health");
  await request(page, "List users");
  await send(page);
  await shot(page, "collections-seitenleiste");
});

test("collections-neu", async ({ page }) => {
  await open(page);
  await page.keyboard.press(`${MOD}+n`);
  await expect(page.getByTestId("new-dialog")).toBeVisible();
  await shot(page, "collections-neu");
});

test("collections-tabgruppen", async ({ page }) => {
  await open(page);
  await request(page, "Health");
  await request(page, "List users");
  await request(page, "Create user");
  await page.getByTestId("tab").filter({ hasText: "List users" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Add tab to new group" }).click();
  await page.keyboard.type("Users");
  await page.keyboard.press("Enter");
  await page.getByTestId("tab").filter({ hasText: "Create user" }).click({ button: "right" });
  await page.getByRole("menuitem", { name: "Add tab to group" }).hover();
  await page.getByRole("menuitem", { name: "Users" }).click();
  await page.getByTestId("tab-body").click();
  await shot(page, "collections-tabgruppen", { x: 0, y: 0, width: 1440, height: 380 });
});
