// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 8, strand E (AI assistant), against the mock backend: off until
// configured, settings, "Create with AI" (confirm, request appears unsaved), "Explain" on a
// failed response, "Suggest tests" applied unsaved. The mock answers with canned proposals;
// no call leaves the page.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend, RequestSpec } from "@requesttailor/ui";

type W = { __rt: { backend: MockBackend } };

const MOD = process.platform === "darwin" ? "Meta" : "Control";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("tree-request").first()).toBeVisible();
}

async function palette(page: Page, query: string) {
  await page.keyboard.press(`${MOD}+k`);
  const p = page.getByTestId("palette");
  await expect(p).toBeVisible();
  await page.getByTestId("palette-input").fill(query);
  return p;
}

async function runCommand(page: Page, label: string) {
  const p = await palette(page, label);
  await p.getByTestId("palette-option").filter({ hasText: label }).first().click();
  await expect(p).toBeHidden();
}

async function configure(page: Page) {
  await runCommand(page, "AI assistant: settings");
  const dialog = page.getByTestId("ai-settings");
  await expect(dialog).toBeVisible();
  // No provider is chosen in advance; the privacy note is there.
  await expect(page.getByTestId("ai-provider")).toHaveValue("");
  await expect(page.getByTestId("ai-privacy")).toContainText("replaced by ***");
  await expect(page.getByTestId("ai-save")).toBeDisabled();
  await page.getByTestId("ai-provider").selectOption("anthropic");
  await expect(page.getByTestId("ai-model")).toHaveValue("claude-opus-5-5");
  // Anthropic needs a key.
  await expect(page.getByTestId("ai-save")).toBeDisabled();
  await page.getByTestId("ai-key").fill("sk-test-key");
  await page.getByTestId("ai-save").click();
  await expect(dialog).toBeHidden();
}

/** Makes the mock answer `path` with `status`. */
async function failWith(page: Page, path: string, status: number) {
  await page.evaluate(
    ({ path, status }) => {
      const b = (window as unknown as W).__rt.backend;
      const send = b.send.bind(b);
      b.send = async (p, input) => {
        const r = await send(p, input);
        if (p === path && r.report.response) r.report.response.status = status;
        return r;
      };
    },
    { path, status },
  );
}

test("off until configured: no buttons, only the settings entry in the palette", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await failWith(page, "health.yml", 500);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("500");
  await expect(page.getByTestId("ai-explain-button")).toHaveCount(0);
  await page.getByTestId("tab-scripts").click();
  await expect(page.getByTestId("scripts-tab")).toBeVisible();
  await expect(page.getByTestId("ai-suggest-tests")).toHaveCount(0);
  const p = await palette(page, "AI");
  await expect(p.getByTestId("palette-option").filter({ hasText: "AI assistant: settings" })).toHaveCount(1);
  await expect(p.getByTestId("palette-option").filter({ hasText: "AI: create request" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  // The key never comes back to the page: only whether one is set.
  await configure(page);
  await runCommand(page, "AI assistant: settings");
  await expect(page.getByTestId("ai-key-set")).toBeVisible();
  await expect(page.getByTestId("ai-key")).toHaveValue("");
  await page.getByTestId("ai-cancel").click();
  await expect(page.getByTestId("ai-explain-button")).toBeVisible();
});

test("create with AI: the proposal is confirmed and appears unsaved in the chosen folder", async ({ page }) => {
  await openCollection(page);
  await configure(page);
  await runCommand(page, "AI: new request from a description or snippet");
  const dialog = page.getByTestId("ai-create");
  await expect(dialog).toBeVisible();
  await page.getByTestId("ai-create-description").fill("Create an order");
  await page.getByTestId("ai-create-folder").selectOption("users");
  await page.getByTestId("ai-create-propose").click();
  // Wave 16: what will be sent first (first use of the profile), then the proposal as a
  // suggestion to apply or discard.
  await page.getByTestId("ai-preview-send").click();
  await expect(dialog).toBeHidden();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toContainText("POST");
  await expect(view).toContainText("{{baseUrl}}/order");
  // Nothing exists before applying.
  const before = await page.evaluate(() => (window as unknown as W).__rt.backend.dirty());
  expect(before).toEqual([]);
  await view.getByTestId("suggest-apply").click();

  const path = "users/create-an-order.yml";
  await expect(page.getByTestId("tree-request").filter({ hasText: "Create an order" })).toBeVisible();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual([path]);
  const spec = (await page.evaluate((p) => (window as unknown as W).__rt.backend.getRequest(p), path)) as RequestSpec;
  expect([spec.method, spec.url, spec.auth.type, spec.body.type]).toEqual(["POST", "{{baseUrl}}/order", "inherit", "raw"]);
  await expect(page.getByTestId("request-editor")).toBeVisible();
  const asked = await page.evaluate(() => (window as unknown as W).__rt.backend.ai.asked);
  expect(asked).toEqual([{ task: "create-request", description: "Create an order", folder: "users" }]);
});

test("explain a failed response", async ({ page }) => {
  await openCollection(page);
  await configure(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  // Wave 16: a response that worked can be summarised (its own task and test in e2e/ai-free).
  await expect(page.getByTestId("ai-explain-button")).toBeVisible();

  await failWith(page, "health.yml", 401);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("401");
  await page.getByTestId("ai-explain-button").click();
  const dialog = page.getByTestId("ai-explain");
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("ai-explain-text")).toContainText("The server refused the request.");
  // Markdown is rendered as text: bold and a numbered list.
  await expect(page.getByTestId("ai-explain-text").locator("strong").first()).toHaveText("The server refused the request.");
  await expect(page.getByTestId("ai-explain-text").locator("ol li")).toHaveCount(2);
  await page.getByTestId("ai-explain-close").click();
  await expect(dialog).toBeHidden();
  const asked = await page.evaluate(() => (window as unknown as W).__rt.backend.ai.asked);
  expect(asked).toEqual([{ task: "explain-error", path: "health.yml", response: expect.any(Number) }]);
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual([]);
});

test("suggest tests: shown, then applied unsaved", async ({ page }) => {
  await openCollection(page);
  await configure(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("tab-scripts").click();
  // Needs a response first.
  await expect(page.getByTestId("ai-suggest-tests")).toBeDisabled();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.getByTestId("ai-suggest-tests").click();
  const dialog = page.getByTestId("ai-tests");
  await expect(dialog).toBeVisible();
  await expect(page.getByTestId("ai-tests-script")).toContainText("expect(rt.response.status).to.equal(200)");
  await expect(page.getByTestId("ai-tests-assertions").locator("li")).toHaveCount(2);
  // Nothing changes before "Add to request".
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual([]);
  await page.getByTestId("ai-tests-apply").click();
  await expect(dialog).toBeHidden();

  await expect.poll(() => page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual(["health.yml"]);
  const spec = (await page.evaluate(() => (window as unknown as W).__rt.backend.getRequest("health.yml"))) as RequestSpec;
  expect(spec.scripts.map((s) => s.phase)).toEqual(["tests"]);
  expect(spec.assertions.map((a) => `${a.expression} ${a.operator}`)).toEqual(["res.status eq", "res.body isJson"]);
  // The editor shows the added tests script.
  await expect(page.getByTestId("script-tests")).toContainText("status is 200");
  // The palette offers the tasks for the open request now.
  const p = await palette(page, "AI:");
  await expect(p.getByTestId("palette-option").filter({ hasText: "AI: explain this response" })).toHaveCount(1);
  await expect(p.getByTestId("palette-option").filter({ hasText: "AI: suggest tests" })).toHaveCount(1);
});
