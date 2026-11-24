// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand A: `AppExtension.onExchange` after every send, and `SuggestedEdit.batch`
// (one dialog, one undo step), with the demo17 extension of the dev server. Mock backend only.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const URL = "/?backend=mock&extensions=demo17";

async function openCollection(page: Page) {
  await page.goto(URL);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function runCommand(page: Page, label: string) {
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill(label);
  await palette.getByRole("option").first().click();
}

const demoLog = (page: Page) => page.evaluate(() => (window as unknown as { __demo17?: string[] }).__demo17 ?? []);

test("onExchange fires once per send with the request as written", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
  expect(await demoLog(page)).toEqual([]);
  await page.getByTestId("send").click();
  await expect.poll(() => demoLog(page)).toHaveLength(1);
  expect((await demoLog(page))[0]).toMatch(/^users\/list-users\.yml:\d{3}:template$/);
  await page.getByTestId("send").click();
  await expect.poll(() => demoLog(page)).toHaveLength(2);
});

test("a batch suggestion shows both requests in one dialog and applies with one undo", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await runCommand(page, "Demo17: chain");
  const view = page.getByTestId("suggested-edit");
  await expect(view).toBeVisible();
  await expect(view).toContainText("Pass the chained value on");
  const blocks = view.getByTestId("suggest-block");
  await expect(blocks).toHaveCount(2);
  await expect(blocks.first()).toContainText("users/list-users.yml");
  await expect(blocks.nth(1)).toContainText("users/create-user.yml");

  await view.getByTestId("suggest-apply").click();
  await expect(view).toHaveCount(0);
  expect(await demoLog(page)).toContain("batch:true");
  const headers = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: { getRequest(p: string): Promise<{ headers: { name: string }[] }> } } }).__rt.backend;
    return [(await b.getRequest("users/list-users.yml")).headers, (await b.getRequest("users/create-user.yml")).headers].map((h) => h.some((x) => x.name === "X-Chain"));
  });
  expect(headers).toEqual([true, true]);

  // One undo step puts both back.
  await runCommand(page, "Undo suggestion");
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const b = (window as unknown as { __rt: { backend: { getRequest(p: string): Promise<{ headers: { name: string }[] }> } } }).__rt.backend;
        return [(await b.getRequest("users/list-users.yml")).headers, (await b.getRequest("users/create-user.yml")).headers].map((h) => h.some((x) => x.name === "X-Chain"));
      }),
    )
    .toEqual([false, false]);
});

test("discarding the batch changes nothing", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await runCommand(page, "Demo17: chain");
  await page.getByTestId("suggested-edit").getByTestId("suggest-discard").click();
  expect(await demoLog(page)).toContain("batch:false");
  expect(await page.evaluate(async () => (await (window as unknown as { __rt: { backend: { dirty(): Promise<string[]> } } }).__rt.backend.dirty()).length)).toBe(0);
});
