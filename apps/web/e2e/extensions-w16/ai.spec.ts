// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand C: `ctx.ai` for extensions: the preview before a call, cancelling it, and
// aborting a running call. The provider is faked inside the page (the mock backend's AI calls
// are replaced), so nothing leaves the machine and no network is needed.

import { expect, test, type Page } from "@playwright/test";

import { openCollection, openRequest } from "./helpers";

type Fake = { calls: string[]; cancelled: string[]; confirmed: string[] };
type W = Window & { __fakeAi: Fake; __rt: { backend: { ai: Record<string, unknown> } } };

async function fakeProvider(page: Page) {
  await page.evaluate(() => {
    const w = window as unknown as W;
    const fake: Fake = { calls: [], cancelled: [], confirmed: [] };
    w.__fakeAi = fake;
    let pending: { id: string; reject: (e: unknown) => void } | null = null;
    const ai = w.__rt.backend.ai;
    ai.policy = async () => ({ file_off: false, device_off: false });
    ai.preview = async () => ({
      profile: "demo",
      provider: "openai-compatible",
      model: "local",
      endpoint: "http://127.0.0.1:1/v1",
      call: { system: "You are a demo.", messages: [{ role: "user", content: "About a request" }], max_tokens: 100 },
      masked: 0,
      bytes: 42,
      required: true,
    });
    ai.confirmPreview = async (profile: string) => void fake.confirmed.push(profile);
    ai.complete = (request: { id: string }) =>
      new Promise((_resolve, reject) => {
        fake.calls.push(request.id);
        pending = { id: request.id, reject };
      });
    ai.cancel = async (id: string) => {
      fake.cancelled.push(id);
      if (pending?.id === id) pending.reject(Object.assign(new Error("cancelled"), { kind: "cancelled" }));
    };
  });
}

test("the preview comes first; cancelling it sends nothing", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await fakeProvider(page);
  await page.getByTestId("assistant-toggle").click();
  await page.getByTestId("demo16-ask").click();
  const dialog = page.getByTestId("extension-dialog");
  await expect(dialog.getByTestId("ai-preview")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("demo16-problem")).toContainText("cancelled");
  expect(await page.evaluate(() => (window as unknown as W).__fakeAi.calls)).toEqual([]);
});

test("confirming the preview sends; Stop aborts the running call", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await fakeProvider(page);
  await page.getByTestId("assistant-toggle").click();
  await page.getByTestId("demo16-ask").click();
  await page.getByTestId("extension-dialog").getByRole("button", { name: /^Send/ }).click();
  await expect(page.getByTestId("demo16-stop")).toBeVisible();
  const state = () => page.evaluate(() => (window as unknown as W).__fakeAi);
  expect((await state()).calls).toHaveLength(1);
  // The preview was required (first use): the confirmation is remembered.
  expect((await state()).confirmed).toEqual(["demo"]);

  await page.getByTestId("demo16-stop").click();
  await expect(page.getByTestId("demo16-problem")).toContainText("cancelled");
  const after = await state();
  expect(after.cancelled).toEqual(after.calls);
});
