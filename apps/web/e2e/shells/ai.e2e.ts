// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand A with the real core (rt-wasm): settings, the exact masked call reaching a
// fake provider (testdata/ai/fake-provider.mjs, never a real one), cancel, and opt-out.
// Needs `?extensions=demo` (dev server) for the "Demo: ask the AI" command.

import { expect, test, type Page } from "@playwright/test";

import { AI as AI_ROOT, fakePicker, fillFolder, sampleCollection } from "./env";

// Its own lane of the fake provider: other test files run in parallel and must not see these calls.
const AI = `${AI_ROOT}/lane/ai`;

type Backend = import("@requesttailor/ui").Backend;
type W = { __rt: { backend: Backend }; __demoAi?: { text?: string; error?: string } };
// The fake provider keeps one script and one call log: these tests run one after another.
test.describe.configure({ mode: "serial" });

const MOD = process.platform === "darwin" ? "Meta" : "Control";
const SECRET = ["ai", "shell", "9c1d"].join("-");

const calls = async (page: Page) => (await (await page.request.get(`${AI}/__calls`)).json()) as { path: string; headers: Record<string, string>; body: unknown }[];

async function setup(page: Page) {
  await page.request.post(`${AI}/__reset`);
  await page.request.post(`${AI}/__script`, { data: { replies: [{ text: "hello from the fake" }] } });
  await fakePicker(page);
  await page.goto("/?extensions=demo");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.evaluate(
    async ({ ai, secret }) => {
      const b = (window as unknown as W).__rt.backend;
      await b.secrets.set("Local", "token", secret);
      await b.ai.setSettings({
        profiles: [{ id: "default", label: "Fake", provider: "anthropic", model: "fake-model", base_url: `${ai}/anthropic` }],
        roles: {},
        preview: "first-use",
      });
      await b.ai.setProfileKey("default", "fake-key");
    },
    { ai: AI, secret: SECRET },
  );
}

async function ask(page: Page) {
  await page.evaluate((s) => ((window as unknown as { __demoToken?: string }).__demoToken = s), SECRET);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Demo: ask the AI");
  await page.getByTestId("palette-option").filter({ hasText: "Demo: ask the AI" }).first().click();
}

test("the preview is the call: masked, sent once, with the profile's key", async ({ page }) => {
  await setup(page);
  await ask(page);
  await expect(page.getByTestId("ai-preview")).toBeVisible();
  await page.getByTestId("ai-preview-send").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.text), { timeout: 20_000 }).toBe("hello from the fake");
  const seen = await calls(page);
  expect(seen).toHaveLength(1);
  expect(seen[0]!.path).toBe("/anthropic/v1/messages");
  expect(seen[0]!.headers["x-api-key"]).toBe("fake-key");
  expect(JSON.stringify(seen[0]!.body)).toContain("token=***");
  expect(JSON.stringify(seen[0]!.body)).not.toContain(SECRET);
});

test("cancelling the preview sends nothing", async ({ page }) => {
  await setup(page);
  await ask(page);
  await expect(page.getByTestId("ai-preview")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error), { timeout: 20_000 }).toBe("cancelled");
  expect(await calls(page)).toEqual([]);
});

test("opt-out on this device: no call is built", async ({ page }) => {
  await setup(page);
  await page.evaluate(() => (window as unknown as W).__rt.backend.ai.setDeviceOptOut(true));
  await ask(page);
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error), { timeout: 20_000 }).toBe("opted-out");
  await expect(page.getByTestId("ai-preview")).toHaveCount(0);
  expect(await calls(page)).toEqual([]);
});

test("a provider error comes back without the key", async ({ page }) => {
  await setup(page);
  await page.request.post(`${AI}/__script`, { data: { replies: [{ status: 429, error: "rate limited" }] } });
  await ask(page);
  await page.getByTestId("ai-preview-send").click();
  await expect.poll(() => page.evaluate(() => (window as unknown as W).__demoAi?.error), { timeout: 20_000 }).toBeTruthy();
  expect(await page.evaluate(() => JSON.stringify((window as unknown as W).__demoAi))).not.toContain("fake-key");
});
