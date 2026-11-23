// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand B with the real core (rt-wasm): the app's own assistant tasks against the
// fake provider (testdata/ai/fake-provider.mjs, never a real one). The call that arrives is
// the task's prompt with the context and no secret value; its answer comes back as a
// suggestion (visible, unsaved, undone); an invalid answer is refused, not repaired; a cURL
// snippet needs no provider at all.

import { expect, test, type Page } from "@playwright/test";

import { AI, fakePicker, fillFolder, sampleCollection } from "./env";

type Backend = import("@requesttailor/ui").Backend;
type W = { __rt: { backend: Backend } };
// The fake provider keeps one script and one call log: these tests run one after another.
test.describe.configure({ mode: "serial" });

const SECRET = ["ai", "free", "5e7b"].join("-");

const calls = async (page: Page) => (await (await page.request.get(`${AI}/__calls`)).json()) as { path: string; headers: Record<string, string>; body: unknown }[];
const reply = (page: Page, text: string) => page.request.post(`${AI}/__script`, { data: { replies: [{ text }] } });
const fenced = (o: unknown) => "```json\n" + JSON.stringify(o) + "\n```";

async function setup(page: Page) {
  await page.request.post(`${AI}/__reset`);
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  // Settings and the profile's key, then a fresh page: the assistant is read when the app starts.
  await page.evaluate(
    async ({ ai }) => {
      const b = (window as unknown as W).__rt.backend;
      await b.ai.setSettings({
        profiles: [{ id: "default", label: "Fake", provider: "anthropic", model: "fake-model", base_url: `${ai}/anthropic` }],
        roles: {},
        preview: "first-use",
      });
      await b.ai.setProfileKey("default", "fake-key");
    },
    { ai: AI },
  );
  await page.reload();
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.evaluate((s) => (window as unknown as W).__rt.backend.secrets.set("Local", "token", s), SECRET);
}

async function sendHealth(page: Page) {
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200", { timeout: 20_000 });
}

test("assertions from the response: the call is masked, the answer is a diff, applied unsaved and undone", async ({ page }) => {
  await setup(page);
  await reply(
    page,
    fenced({
      text: "Status and type.",
      assertions: [
        { expression: "res.status", operator: "eq", value: "200", description: "status is OK" },
        { expression: "res.body", operator: "looksGood" },
      ],
    }),
  );
  await sendHealth(page);
  await page.getByTestId("tab-assertions").click();
  await page.getByTestId("ai-suggest-assertions").click();
  // First use of the profile: the exact call is shown before it leaves.
  await expect(page.getByTestId("ai-preview")).toBeVisible();
  expect(await calls(page)).toEqual([]);
  await page.getByTestId("ai-preview-send").click();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toBeVisible({ timeout: 20_000 });
  const seen = await calls(page);
  expect(seen).toHaveLength(1);
  expect(seen[0]!.path).toBe("/anthropic/v1/messages");
  const body = JSON.stringify(seen[0]!.body);
  expect(body).toContain("suggest declarative assertions");
  expect(body).toMatch(/status[^0-9]{1,8}200/);
  expect(body).not.toContain(SECRET);

  // The invalid assertion was dropped, not repaired: one block for the one valid assertion.
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual([]);
  await view.getByTestId("suggest-apply").click();
  await expect(page.getByTestId("assertion-row")).toHaveCount(1);
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual(["health.yml"]);
  await page.getByTestId("suggest-undo").click();
  await expect(page.getByTestId("assertion-row")).toHaveCount(0);
});

test("an answer without a usable assertion is refused with a reason, nothing changes", async ({ page }) => {
  await setup(page);
  await reply(page, fenced({ text: "x", assertions: [{ expression: "res.body", operator: "looksGood" }] }));
  await sendHealth(page);
  await page.getByTestId("tab-assertions").click();
  await page.getByTestId("ai-suggest-assertions").click();
  await page.getByTestId("ai-preview-send").click();
  await expect(page.getByText("no usable assertion")).toBeVisible({ timeout: 20_000 });
  await expect(page.getByTestId("suggested-edit")).toHaveCount(0);
  expect(await page.evaluate(() => (window as unknown as W).__rt.backend.dirty())).toEqual([]);
});

test("a cURL snippet is read by the core without any provider call, even with no assistant set up", async ({ page }) => {
  await page.request.post(`${AI}/__reset`);
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  const answer = await page.evaluate(() =>
    (window as unknown as W).__rt.backend.ai.ask(
      { task: "request-from-snippet", snippet: "curl -X PUT 'https://api.test/items/1?x=1' -H 'Accept: application/json' -d 'name=a'", folder: "" },
      { environment: null, vars: [] },
    ),
  );
  expect(answer.request).toMatchObject({ method: "PUT", url: "https://api.test/items/1?x=1", auth: { type: "inherit" } });
  expect(answer.text).toContain("without a model");
  // A snippet that is not cURL needs the assistant, which is not set up here.
  await expect(
    page.evaluate(() =>
      (window as unknown as W).__rt.backend.ai.ask({ task: "request-from-snippet", snippet: "fetch('https://api.test')", folder: "" }, { environment: null, vars: [] }),
    ),
  ).rejects.toThrow(/no AI provider is set up/);
  expect(await calls(page)).toEqual([]);
});

test("names: strict validation drops what cannot be a name", async ({ page }) => {
  await setup(page);
  await reply(page, fenced({ names: ["List health", "a/b", "", "List health", "Health check"] }));
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.keyboard.press("ControlOrMeta+k");
  await page.getByTestId("palette-input").fill("AI: suggest a name for this request");
  await page.getByTestId("palette-option").filter({ hasText: "AI: suggest a name for this request" }).first().click();
  await page.getByTestId("ai-preview-send").click();
  await expect(page.getByTestId("ai-name")).toHaveText(["List health", "Health check"], { timeout: 20_000 });
});
