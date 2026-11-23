// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand B against the mock backend: the app's own assistant tasks. Off until set up
// and off per collection; a request from a cURL command without a model; a request from a
// description after the "what will be sent" preview; assertions from the response, a response
// explained, names and a description draft. Every proposal is a suggestion: visible, unsaved,
// applied on request, undone where it can be. No call leaves the page.

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type W = { __rt: { backend: MockBackend } };

const MOD = "ControlOrMeta";

const backend = <T>(page: Page, f: (b: MockBackend) => T | Promise<T>) =>
  page.evaluate(`(${f.toString()})(window.__rt.backend)`) as Promise<T>;

async function open(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

/** Sets the assistant up through its settings dialog, as a user does (Anthropic, a key; the
 *  mock never calls it). The first use of the profile shows "what will be sent". */
async function setUp(page: Page) {
  await command(page, "AI assistant: settings");
  const dialog = page.getByTestId("ai-settings");
  await expect(dialog).toBeVisible();
  await page.getByTestId("ai-provider").selectOption("anthropic");
  await page.getByTestId("ai-key").fill("sk-test-key");
  await page.getByTestId("ai-save").click();
  await expect(dialog).toBeHidden();
}

/** The collection's own switch (`opencollection.yml`), then the settings dialog is opened and
 *  closed, which is when the app reads the switch again. */
async function turnOffInFile(page: Page) {
  await backend(page, (b) => void (b.ai.fileOff = true));
  await command(page, "AI assistant: settings");
  await page.getByTestId("ai-cancel").click();
}

async function command(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill(label);
  await page.getByTestId("palette-option").filter({ hasText: label }).first().click();
}

const dirty = (page: Page) => backend(page, (b) => b.dirty());
const asked = (page: Page) => backend(page, (b) => b.ai.asked.map((t) => t.task));
const sent = (page: Page) => backend(page, (b) => b.ai.sent.length);
const suggestion = (page: Page) => page.getByTestId("suggested-edit");

async function sendHealth(page: Page) {
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

test("off until set up: no tile, no palette entries, no buttons, no call", async ({ page }) => {
  await open(page);
  await sendHealth(page);
  await expect(page.getByTestId("ai-explain-button")).toHaveCount(0);
  await page.getByTestId("tab-assertions").click();
  await expect(page.getByTestId("assertions-tab")).toBeVisible();
  await expect(page.getByTestId("ai-suggest-assertions")).toHaveCount(0);
  await page.keyboard.press(`${MOD}+n`);
  await expect(page.getByTestId("new-dialog")).toBeVisible();
  await expect(page.locator('[data-testid=new-tile][data-key="ai-request"]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("AI");
  await expect(page.getByTestId("palette-option").filter({ hasText: "AI:" })).toHaveCount(0);
  expect(await asked(page)).toEqual([]);
});

test("a cURL command becomes a request without a model, unsaved", async ({ page }) => {
  await open(page);
  await setUp(page);
  await page.keyboard.press(`${MOD}+n`);
  const tile = page.locator('[data-testid=new-tile][data-key="ai-request"]');
  await expect(tile).toContainText("From description or snippet");
  await tile.click();
  const dialog = page.getByTestId("ai-create");
  await expect(dialog).toBeVisible();
  await page.getByTestId("ai-create-folder").selectOption("users");
  await page.getByTestId("ai-create-description").fill(`curl -X POST https://api.test/orders -H 'Content-Type: application/json' -d '{"sku":"a-1"}'`);
  // Pasting a cURL command switches to the snippet mode and says that no model is used.
  await expect(page.getByTestId("ai-create-mode-snippet")).toBeChecked();
  await expect(page.getByTestId("ai-create-local")).toContainText("No model is called");
  await page.getByTestId("ai-create-propose").click();
  // No "what will be sent": nothing is sent.
  await expect(page.getByTestId("ai-preview")).toHaveCount(0);
  await expect(suggestion(page)).toBeVisible();
  await expect(suggestion(page).getByTestId("suggest-block").first()).toBeVisible();
  expect(await dirty(page)).toEqual([]);
  await suggestion(page).getByTestId("suggest-apply").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "POST api.test/orders" })).toBeVisible();
  await expect.poll(() => dirty(page).then((d) => d.length)).toBe(1);
  expect(await sent(page)).toBe(0);
  expect(await asked(page)).toEqual(["request-from-snippet"]);
});

test("a description shows what will be sent first; cancelling sends nothing", async ({ page }) => {
  await open(page);
  await setUp(page);
  await backend(page, (b) => void (b.ai.secrets = ["hunter2-secret"]));
  await command(page, "AI: new request from a description or snippet");
  await page.getByTestId("ai-create-description").fill("create an order, password hunter2-secret");
  await page.getByTestId("ai-create-propose").click();
  const preview = page.getByTestId("ai-preview");
  await expect(preview).toBeVisible();
  await expect(page.getByTestId("ai-preview-masked")).toContainText("secret value");
  await expect(preview).not.toContainText("hunter2-secret");
  await page.getByTestId("ai-preview-cancel").click();
  // Nothing left, nothing was created, and the dialog is back with what was typed.
  expect(await asked(page)).toEqual([]);
  await expect(page.getByTestId("ai-create-description")).toHaveValue("create an order, password hunter2-secret");
  await page.getByTestId("ai-create-propose").click();
  await page.getByTestId("ai-preview-send").click();
  await expect(suggestion(page)).toBeVisible();
  await suggestion(page).getByTestId("suggest-discard").click();
  expect(await dirty(page)).toEqual([]);
  expect(await asked(page)).toEqual(["create-request"]);
});

test("assertions from the response: a diff, applied unsaved, undone", async ({ page }) => {
  await open(page);
  await setUp(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("tab-assertions").click();
  await expect(page.getByTestId("ai-suggest-assertions")).toBeDisabled();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.getByTestId("ai-suggest-assertions").click();
  await page.getByTestId("ai-preview-send").click();
  const view = suggestion(page);
  await expect(view).toBeVisible();
  await expect(view).toContainText("Assertions from the response");
  expect(await dirty(page)).toEqual([]);
  await view.getByTestId("suggest-apply").click();
  await expect.poll(() => dirty(page)).toEqual(["health.yml"]);
  await expect(page.getByTestId("assertion-row")).toHaveCount(2);
  await page.getByTestId("suggest-undo").click();
  await expect(page.getByTestId("assertion-row")).toHaveCount(0);
  await expect.poll(() => dirty(page)).toEqual([]);
});

test("Explain on a response that worked: a short summary, nothing changes", async ({ page }) => {
  await open(page);
  await setUp(page);
  await sendHealth(page);
  await page.getByTestId("ai-explain-button").click();
  await page.getByTestId("ai-preview-send").click();
  await expect(page.getByTestId("ai-result-text")).toContainText("Returns 3 items");
  expect(await asked(page)).toEqual(["explain-response"]);
  await page.getByTestId("ai-result-close").click();
  expect(await dirty(page)).toEqual([]);
});

test("a name for the request: choose, see the diff, apply, undo", async ({ page }) => {
  await open(page);
  await setUp(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await command(page, "AI: suggest a name for this request");
  await page.getByTestId("ai-preview-send").click();
  await expect(page.getByTestId("ai-name")).toHaveCount(3);
  await page.getByTestId("ai-name-use").first().click();
  await expect(suggestion(page).getByTestId("suggest-block")).toHaveCount(1);
  await suggestion(page).getByTestId("suggest-apply").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "List open orders" })).toBeVisible();
  await page.getByTestId("suggest-undo").click();
  await expect(page.getByTestId("tree-request").filter({ hasText: "List open orders" })).toHaveCount(0);
});

test("a description draft is read, then reviewed as a Markdown file, unsaved", async ({ page }) => {
  await open(page);
  await setUp(page);
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await command(page, "AI: draft a description for this request");
  await page.getByTestId("ai-preview-send").click();
  await expect(page.getByTestId("ai-docs-text")).toContainText("open orders");
  expect(await dirty(page)).toEqual([]);
  await page.getByTestId("ai-docs-save").click();
  await expect(suggestion(page)).toContainText("health.md");
  await suggestion(page).getByTestId("suggest-apply").click();
  await expect.poll(() => dirty(page)).toContain("health.md");
});

test("AI is off for this collection: the buttons say so and nothing is asked", async ({ page }) => {
  await open(page);
  await setUp(page);
  await sendHealth(page);
  await expect(page.getByTestId("ai-explain-button")).toBeEnabled();
  await turnOffInFile(page);
  await expect(page.getByTestId("ai-explain-button")).toBeDisabled();
  await expect(page.getByTestId("ai-explain-button")).toHaveAttribute("title", "AI is off for this collection");
  await page.getByTestId("tab-assertions").click();
  await expect(page.getByTestId("ai-suggest-assertions")).toBeDisabled();
  await expect(page.getByTestId("assertions-tab")).toContainText("AI is off for this collection");
  // No entry in the palette or the New dialog, and nothing was asked.
  await page.keyboard.press(`${MOD}+n`);
  await expect(page.getByTestId("new-dialog")).toBeVisible();
  await expect(page.locator('[data-testid=new-tile][data-key="ai-request"]')).toHaveCount(0);
  await page.keyboard.press("Escape");
  expect(await asked(page)).toEqual([]);
  expect(await sent(page)).toBe(0);
});
