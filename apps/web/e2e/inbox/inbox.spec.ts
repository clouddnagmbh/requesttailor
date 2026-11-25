// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 18, strand E: the webhook inbox view (palette "Inbox: Start"), the list
// of what arrived, the detail with headers and body, search, "Clear", "Make request" through the
// additions dialog (secrets as variables without a value) and no horizontal scrolling at 900 px.
// Mock backend: nothing listens; the page hands requests to `window.__rtMockInbox`.

import { expect, test, type Page } from "@playwright/test";

const CANARY = "RT-CANARY-inbox-e2e-55d1";

async function openInbox(page: Page, command = "Inbox: Open") {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill(command);
  await palette.getByRole("option").filter({ hasText: command }).first().click();
  await expect(page.getByTestId("inbox")).toBeVisible();
}

/** Hands requests to the mock inbox as if they had arrived. */
async function arrive(page: Page, requests: { method: string; target: string; headers?: [string, string][]; body?: string }[]) {
  await page.evaluate((list) => {
    for (const r of list) window.__rtMockInbox!.push(r);
  }, requests);
}

const rows = (page: Page) => page.getByTestId("inbox-row");

test("nothing listens until Start; the palette starts it and shows the address", async ({ page }) => {
  await openInbox(page);
  await expect(page.getByTestId("inbox-off")).toContainText("Not listening");
  await expect(page.getByTestId("inbox-off")).toContainText("127.0.0.1");
  await expect(page.getByTestId("inbox-url")).toHaveCount(0);
  await page.getByTestId("inbox-toggle").click();
  await expect(page.getByTestId("inbox-url")).toHaveText("http://127.0.0.1:47855/");
  await expect(page.getByTestId("inbox-toggle")).toHaveText("Stop");
  await page.getByTestId("inbox-toggle").click();
  await expect(page.getByTestId("inbox-off")).toBeVisible();
});

test("\"Inbox: Start\" starts the receiver and opens the view", async ({ page }) => {
  await openInbox(page, "Inbox: Start");
  await expect(page.getByTestId("inbox-url")).toBeVisible();
});

test("what arrives is listed newest first, with headers and body in the detail", async ({ page }) => {
  await openInbox(page, "Inbox: Start");
  await expect(page.getByTestId("inbox-empty")).toContainText("Waiting for a request");
  await arrive(page, [
    { method: "POST", target: "/hooks/orders?x=1", headers: [["Content-Type", "application/json"], ["X-Event", "order.created"]], body: '{"order":42,"customer":"Ada"}' },
    { method: "GET", target: "/health" },
  ]);
  await expect(rows(page)).toHaveCount(2);
  await expect(rows(page).first()).toContainText("/health");
  await rows(page).nth(1).click();
  await expect(page.getByTestId("inbox-headers")).toContainText("X-Event");
  await expect(page.getByTestId("inbox-headers")).toContainText("order.created");
  await expect(page.getByTestId("inbox-body-view")).toContainText('"customer": "Ada"');
});

test("the search narrows the list; Clear empties it", async ({ page }) => {
  await openInbox(page, "Inbox: Start");
  await arrive(page, [
    { method: "POST", target: "/a", body: "alpha" },
    { method: "POST", target: "/b", body: "beta" },
    { method: "PUT", target: "/c", body: "gamma" },
  ]);
  await expect(rows(page)).toHaveCount(3);
  await page.getByTestId("inbox-search").fill("beta");
  await expect(rows(page)).toHaveCount(1);
  await expect(page.getByTestId("inbox-count")).toContainText("1 of 3");
  await page.getByTestId("inbox-search").fill("");
  await page.getByTestId("inbox-clear").click();
  await expect(rows(page)).toHaveCount(0);
  await expect(page.getByTestId("inbox-empty")).toBeVisible();
});

test("the list is virtualized and holds 500", async ({ page }) => {
  await openInbox(page, "Inbox: Start");
  await arrive(
    page,
    Array.from({ length: 520 }, (_, i) => ({ method: "POST", target: `/n/${i}` })),
  );
  await expect(page.getByTestId("inbox-count")).toContainText("500 held");
  await expect(page.getByTestId("inbox-dropped")).toContainText("20 older requests dropped");
  const rendered = await rows(page).count();
  expect(rendered).toBeGreaterThan(3);
  expect(rendered).toBeLessThan(80);
  // The newest is first; scrolling brings the oldest.
  await expect(rows(page).first()).toContainText("/n/519");
  await page.getByTestId("inbox-list").evaluate((el) => (el.scrollTop = el.scrollHeight));
  await expect(rows(page).last()).toContainText("/n/20");
});

test("Make request proposes the request and secrets without a value, unsaved", async ({ page }) => {
  await openInbox(page, "Inbox: Start");
  await arrive(page, [
    {
      method: "POST",
      target: `/hooks/orders?api_key=${CANARY}`,
      headers: [
        ["Content-Type", "application/json"],
        ["Authorization", `Bearer ${CANARY}`],
        ["X-Hub-Signature-256", `sha256=${CANARY}`],
        ["Host", "127.0.0.1:47855"],
      ],
      body: '{"order":42}',
    },
  ]);
  await rows(page).first().click();
  await page.getByTestId("inbox-make-request").click();
  const dialog = page.getByTestId("additions-dialog");
  await expect(dialog).toBeVisible();
  await expect(dialog.getByTestId("additions-requests")).toContainText("POST /hooks/orders");
  await expect(dialog.getByTestId("additions-environments")).toContainText("authorization (secret, set it yourself)");
  await expect(dialog.getByTestId("additions-environments")).toContainText("x_hub_signature_256");
  await expect(dialog).not.toContainText(CANARY);
  await dialog.getByRole("button", { name: /add|confirm|ok/i }).first().click();
  await expect(dialog).toHaveCount(0);
  // The collection took the request and the secret variables, none of it with a value from the capture.
  const state = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const tree = await b.tree();
    const requests: string[] = [];
    const walk = (n: import("@requesttailor/ui").TreeNode) => {
      if (n.kind === "request") requests.push(n.path);
      else n.children.forEach(walk);
    };
    walk(tree);
    const added = requests.find((p) => p.includes("hooks"));
    return JSON.stringify({ added, spec: added ? await b.getRequest(added) : null, envs: await b.environments() });
  });
  expect(state).toContain("{{baseUrl}}/hooks/orders");
  expect(state).toContain("{{authorization}}");
  expect(state).not.toContain(CANARY);
});

test("without a collection \"Make request\" is off and says why", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Inbox: Start");
  await palette.getByRole("option").filter({ hasText: "Inbox: Start" }).first().click();
  await expect(page.getByTestId("inbox")).toBeVisible();
  await arrive(page, [{ method: "POST", target: "/x" }]);
  await rows(page).first().click();
  await expect(page.getByTestId("inbox-make-request")).toBeDisabled();
  await expect(page.getByTestId("inbox-detail")).toContainText("Open a collection");
});

test("the fixed reply can be changed; numbers are kept within what the core accepts", async ({ page }) => {
  await openInbox(page);
  await page.getByTestId("inbox-reply-toggle").click();
  await expect(page.getByTestId("inbox-reply")).toContainText("no tunnel");
  await page.getByTestId("inbox-status-code").fill("202");
  await page.getByTestId("inbox-delay").fill("9000");
  await page.getByTestId("inbox-content-type").fill("text/plain");
  await page.getByTestId("inbox-body").fill("ok");
  await page.getByTestId("inbox-reply-save").click();
  await expect(page.getByTestId("inbox-delay")).toHaveValue("5000");
  const settings = await page.evaluate(() => (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend.inbox.settings());
  expect(settings).toMatchObject({ status: 202, delay_ms: 5000, content_type: "text/plain", body: "ok" });
});

test("no horizontal scrolling at 900 px", async ({ page }) => {
  await page.setViewportSize({ width: 900, height: 700 });
  await openInbox(page, "Inbox: Start");
  await arrive(page, [{ method: "POST", target: `/${"long-segment/".repeat(12)}`, headers: [["X-Long", "v".repeat(300)]], body: "x".repeat(400) }]);
  await rows(page).first().click();
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
  const inbox = await page.getByTestId("inbox").evaluate((el) => el.scrollWidth - el.clientWidth);
  expect(inbox).toBeLessThanOrEqual(0);
});
