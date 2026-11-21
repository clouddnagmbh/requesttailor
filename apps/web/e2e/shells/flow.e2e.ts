// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Gate "open folder, create request, send, save" in the browser with the real core:
// WebBackend + rt-wasm + a folder (OPFS behind a fake picker) + rt-testserver.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile, sampleCollection } from "./env";

async function setup(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
}

test("gate in the UI: open folder, create request, type URL, send, save, file on disk", async ({ page }) => {
  await setup(page);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");

  await page.keyboard.press("ControlOrMeta+n");
  await page.getByTestId("new-request-name").fill("Ping");
  await page.getByTestId("new-request-name").press("Enter");
  await expect(page.getByTestId("tree-request").filter({ hasText: "Ping" })).toBeVisible();

  const url = page.getByTestId("url-input").locator(".cm-content");
  await url.click();
  await url.press("ControlOrMeta+a");
  await page.keyboard.type("{{baseUrl}}/echo/ping");

  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  await expect.poll(() => readFolderFile(page, "ping.yml")).toContain("{{baseUrl}}/echo/ping");
});

test("command layer: open folder, create request, set URL, send, save, read the file back", async ({ page }) => {
  await setup(page);
  const result = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const info = await b.openCollection();
    const path = await b.createRequest("", "Ping");
    const spec = await b.getRequest(path);
    await b.updateRequest(path, { ...spec, url: "{{baseUrl}}/echo/ping" });
    const dirtyBefore = await b.dirty();
    const sent = await b.send(path, { environment: "Local", vars: [] });
    const lines = await b.responseLines(sent.response_id!, 0, 100);
    const saved = await b.save();
    return { info, path, dirtyBefore, sent, lines, saved, dirtyAfter: await b.dirty() };
  });
  expect(result.info).toMatchObject({ name: "Shell test", location: "collection" });
  expect(result.info?.id).toMatch(/^[0-9a-f-]{36}$/);
  expect(result.dirtyBefore).toEqual([result.path]);
  expect(result.sent.report.error).toBeNull();
  expect(result.sent.report.response?.status).toBe(200);
  expect(result.sent.report.request?.url).toBe(`${SERVER}/echo/ping`);
  expect(result.lines.lines.join("\n")).toContain("/echo/ping");
  expect(result.saved.map((c) => c.path)).toContain(result.path);
  expect(result.dirtyAfter).toEqual([]);

  const written = await readFolderFile(page, result.path);
  expect(written).toContain("{{baseUrl}}/echo/ping");
});

test("in the UI: open folder, pick a request, send, see 200", async ({ page }) => {
  await setup(page);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
});

test("a 100 MB response is kept whole (not cut at the default body limit)", async ({ page }) => {
  await setup(page);
  const result = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    await b.openCollection();
    const path = await b.createRequest("", "Big");
    const spec = await b.getRequest(path);
    await b.updateRequest(path, { ...spec, url: "{{baseUrl}}/bytes/104857600" });
    const sent = await b.send(path, { environment: "Local", vars: [] });
    const tail = await b.responseBytes(sent.response_id!, 104857600 - 16, 16);
    await b.releaseResponse(sent.response_id!);
    return { size: sent.size, truncated: sent.report.response?.truncated, tail: tail.length };
  });
  expect(result).toEqual({ size: 104857600, truncated: false, tail: 16 });
});

test("a blocked request reaches the UI layer as transport error 'blocked'", async ({ page }) => {
  await setup(page);
  const error = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    await b.openCollection();
    return (await b.send("blocked.yml", { environment: null, vars: [] })).report.error;
  });
  expect(error).toMatchObject({ kind: "transport", message: { kind: "blocked" } });
});

test("secret values are injected at send and never written to files", async ({ page }) => {
  await setup(page);
  const SECRET = "s3cr3t-web-token";
  const result = await page.evaluate(async ({ secret, url }) => {
    const { createWebBackend } = await import(url);
    // In-memory stand-in for the browser secret store (strand D).
    const values = new Map<string, string>();
    const k = (e: string, v: string) => `${e}\u0000${v}`;
    const store = {
      has: async (e: string, v: string) => values.has(k(e, v)),
      get: async (e: string, v: string) => values.get(k(e, v)) ?? null,
      set: async (e: string, v: string, value: string) => void values.set(k(e, v), value),
      delete: async (e: string, v: string) => void values.delete(k(e, v)),
    };
    const b = await createWebBackend({ openSecrets: async () => store });
    await b.openCollection();
    await b.secrets.set("Local", "token", secret);
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", {
      ...spec,
      headers: [{ name: "Authorization", value: "Bearer {{token}}", enabled: true }],
    });
    const sent = await b.send("health.yml", { environment: "Local", vars: [] });
    const body = (await b.responseLines(sent.response_id!, 0, 1000)).lines.join("\n");
    const saved = await b.save();
    return { body, request: JSON.stringify(sent.report.request), saved: JSON.stringify(saved) };
  }, { secret: SECRET, url: "/src/web-backend.ts" });
  expect(result.body).toContain(SECRET);
  expect(result.request).not.toContain(SECRET);
  expect(result.saved).not.toContain(SECRET);
  expect(await readFolderFile(page, "health.yml")).not.toContain(SECRET);
  expect(await readFolderFile(page, "environments/Local.yml")).not.toContain(SECRET);
});

test("the first Open folder after a reload reopens the last folder without a picker", async ({ page }) => {
  await setup(page);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");

  // After a reload the picker must not be needed: access to the OPFS folder stays granted.
  await page.addInitScript(() => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => {
        throw new Error("picker should not open");
      },
    });
  });
  await page.reload();
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await expect(page.getByRole("alert")).toHaveCount(0);
});
