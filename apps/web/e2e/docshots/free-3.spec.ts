// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Documentation screenshots, set Free III: the pages "Prüfen und Beobachten", "Einsicht und
// Aufräumen" and "Begleitdienst" of the website. Runs only with RT_DOC_SHOTS=<dir> and writes one
// PNG per shot there (light theme, 1440x900, DPR 1); convert with cwebp into the website.
//   RT_DOC_SHOTS=/tmp/shots npx playwright test e2e/docshots/free-3.spec.ts
// Mock backend only; every state is driven like the strand specs do.

import { expect, test, type Locator, type Page } from "@playwright/test";

const DIR = process.env.RT_DOC_SHOTS;
const MOD = "ControlOrMeta";

test.skip(!DIR, "set RT_DOC_SHOTS=<dir> to write the documentation screenshots");
test.use({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, colorScheme: "light" });

async function shot(page: Page, id: string, clip?: Locator) {
  await page.mouse.move(1, 1);
  await page.waitForTimeout(400);
  const path = `${DIR}/${id}.png`;
  if (clip) {
    const box = (await clip.boundingBox())!;
    await page.screenshot({ path, clip: { x: box.x, y: box.y, width: box.width, height: box.height } });
  } else {
    await page.screenshot({ path });
  }
}

async function open(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function palette(page: Page, text: string) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(text);
  await dialog.getByRole("option").filter({ hasText: text }).first().click();
}

async function setUrl(page: Page, url: string) {
  await page.evaluate(async (u) => {
    const b = (window as unknown as { __rt: { backend: any } }).__rt.backend;
    const spec = await b.getRequest("users/list-users.yml");
    spec.url = u;
    await b.updateRequest("users/list-users.yml", spec);
  }, url);
}

async function sendList(page: Page, query = "", url?: string) {
  await open(page, query);
  if (url) await setUrl(page, url);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
}

// ---- pruefen ----

test("pruefen-envcompare", async ({ page }) => {
  await open(page);
  await palette(page, "compare environments");
  await expect(page.getByTestId("envcompare-panel")).toBeVisible();
  await page.getByTestId("envcompare-start").click();
  await page.getByTestId("envcompare-confirm-yes").click();
  await expect(page.getByTestId("envcompare-detail")).toBeVisible();
  await shot(page, "pruefen-envcompare");
});

test("pruefen-checks", async ({ page }) => {
  await sendList(page, "", "https://api.example.com/html");
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  await expect(page.getByTestId("checks-panel")).toBeVisible();
  expect(await page.getByTestId("checks-item").count()).toBeGreaterThan(2);
  await shot(page, "pruefen-checks");
});

test("pruefen-lint", async ({ page }) => {
  await open(page);
  await palette(page, "Lint collection");
  await expect(page.getByTestId("lint-score")).toBeVisible();
  await shot(page, "pruefen-lint");
});

test("pruefen-inbox", async ({ page }) => {
  await open(page);
  await palette(page, "Inbox: Start");
  await expect(page.getByTestId("inbox-url")).toBeVisible();
  await page.evaluate(() => {
    const w = window as unknown as { __rtMockInbox: { push(r: unknown): void } };
    w.__rtMockInbox.push({ method: "GET", target: "/health" });
    w.__rtMockInbox.push({ method: "POST", target: "/hooks/payments", headers: [["Content-Type", "application/json"], ["X-Event", "payment.settled"]], body: '{"payment":"pay_1042","amount":1290,"currency":"EUR"}' });
    w.__rtMockInbox.push({ method: "POST", target: "/hooks/orders?source=shop", headers: [["Content-Type", "application/json"], ["X-Event", "order.created"]], body: '{"order":42,"customer":"Ada Example","items":3}' });
  });
  await expect(page.getByTestId("inbox-row")).toHaveCount(3);
  await page.getByTestId("inbox-row").first().click();
  await expect(page.getByTestId("inbox-headers")).toContainText("order.created");
  await shot(page, "pruefen-inbox");
});

// ---- einsicht ----

test("einsicht-trace", async ({ page }) => {
  await sendList(page, "&extensions=demo19&trace=1");
  await page.getByTestId("response-tab-timing").click();
  await expect(page.getByTestId("segments-trace")).toBeVisible();
  await page.getByTestId("segments-trace").scrollIntoViewIfNeeded();
  await page.evaluate(() => document.querySelectorAll("*").forEach((e) => { if (e.scrollHeight > e.clientHeight + 20 && getComputedStyle(e).overflowY !== "visible") e.scrollTop = e.scrollHeight; }));
  await shot(page, "einsicht-trace");
});

test("einsicht-access", async ({ page }) => {
  await open(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: any } }).__rt.backend;
    const spec = await b.getRequest("users/list-users.yml");
    spec.url = "{{baseUrl}}/leak/{{orderId}}";
    await b.updateRequest("users/list-users.yml", spec);
  });
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await palette(page, "Check access");
  await expect(page.getByTestId("access-panel")).toBeVisible();
  await page.getByTestId("access-start").click();
  await expect(page.getByTestId("access-result")).toHaveAttribute("data-verdict", "broken-object-access");
  await shot(page, "einsicht-access");
});

test("einsicht-security", async ({ page }) => {
  await sendList(page, "", "https://api.example.com/cookies/set");
  await page.getByTestId("response-tab-slot-inspect-checks").click();
  await expect(page.getByTestId("checks-owasp").first()).toBeVisible();
  await shot(page, "einsicht-security");
});

test("einsicht-sequence", async ({ page }) => {
  await open(page);
  await palette(page, "Sequence diagram: this session");
  await expect(page.getByTestId("sequence-svg")).toBeVisible();
  await shot(page, "einsicht-sequence");
});

test("einsicht-notebook", async ({ page }) => {
  await open(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await palette(page, "Notebooks");
  await expect(page.getByTestId("notebook-panel")).toBeVisible();
  await page.getByTestId("notebook-run-all").click();
  await expect(page.locator('[data-block="list"]').getByTestId("notebook-chart")).toBeVisible();
  await shot(page, "einsicht-notebook");
});

test("einsicht-debugger", async ({ page }) => {
  await open(page, "&debugger=1");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await palette(page, "Debug scripts");
  await expect(page.getByTestId("debugger-panel")).toBeVisible();
  await page.getByTestId("debugger-gutter").nth(2).click();
  await page.getByTestId("debugger-start").click();
  await expect(page.getByTestId("debugger-status")).toContainText("Stopped at line 3");
  await page.getByTestId("debugger-expression").fill("n * 7");
  await page.getByTestId("debugger-expression").press("Enter");
  await expect(page.getByTestId("debugger-answer")).toContainText("= 42");
  await shot(page, "einsicht-debugger");
});

// ---- begleitdienst ----

test("begleitdienst-angebot", async ({ page }) => {
  await open(page);
  await page.getByTestId("tree-request").filter({ hasText: "Blocked by the browser" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("companion-offer")).toBeVisible();
  await shot(page, "begleitdienst-angebot");
});
