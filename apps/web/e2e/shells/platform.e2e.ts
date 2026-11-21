// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Web shell building blocks that do not need the rest of wave 3: folder reading and writing
// (OPFS), the core (WebAssembly) loading and sending to rt-testserver, the "blocked" error
// kind, and browsers without the File System Access API.

import { expect, test } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile, sampleCollection } from "./env";

// Modules of the app, imported inside the page through the Vite dev server.
const DIRECTORY = "/src/fs/directory.ts";
const WASM = "/src/wasm.ts";

test("reads a folder with the skip rules and writes changes into it", async ({ page }) => {
  await page.goto("/?backend=mock");
  await fillFolder(page, { ...sampleCollection(), "files/big.bin": "x".repeat(16 * 1024 * 1024 + 1) });
  const result = await page.evaluate(async (url) => {
    const fs = await import(url);
    const root = await (await navigator.storage.getDirectory()).getDirectoryHandle("collection");
    const files: Map<string, Uint8Array> = await fs.readDirectory(root);
    await fs.writeChanges(root, [
      { path: "health.yml", text: "changed\n" },
      { path: "users/admin/new.yml", text: "new\n" },
    ]);
    const rejected: string[] = [];
    for (const bad of ["../x.yml", "/x.yml", "a//b.yml", "a\\b.yml", "./x.yml", ""]) {
      try {
        await fs.writeChanges(root, [{ path: "ok.yml", text: "ok" }, { path: bad, text: "x" }]);
      } catch (e) {
        if ((e as { kind?: string }).kind === "shell") rejected.push(bad);
      }
    }
    return { paths: [...files.keys()], rejected };
  }, DIRECTORY);
  expect(result.paths).toEqual(["blocked.yml", "environments/Local.yml", "health.yml", "opencollection.yml"]);
  expect(result.rejected).toHaveLength(6);
  expect(await readFolderFile(page, "health.yml")).toBe("changed\n");
  expect(await readFolderFile(page, "users/admin/new.yml")).toBe("new\n");
  // Paths are checked before anything is written.
  expect(await readFolderFile(page, "ok.yml")).toBeNull();
});

test("the core loads in the page and sends to rt-testserver", async ({ page }) => {
  await page.goto("/?backend=mock");
  const report = await page.evaluate(
    async ({ url, files }) => {
      const { loadCore } = await import(url);
      const core = await loadCore();
      return core.send(new Map(Object.entries(files)), "health.yml", { environment: "Local" });
    },
    { url: WASM, files: sampleCollection() },
  );
  expect(report.error).toBeNull();
  expect(report.response.status).toBe(200);
  expect(report.request.url).toBe(`${SERVER}/echo/health`);
});

test("a request the browser refuses arrives as transport error kind 'blocked'", async ({ page }) => {
  await page.goto("/?backend=mock");
  const report = await page.evaluate(
    async ({ url, files }) => {
      const { loadCore } = await import(url);
      const core = await loadCore();
      return core.send(new Map(Object.entries(files)), "blocked.yml", {});
    },
    { url: WASM, files: sampleCollection() },
  );
  expect(report.response).toBeNull();
  expect(report.error).toMatchObject({ kind: "transport", message: { kind: "blocked" } });
});

test("without the File System Access API, Open folder explains why instead of crashing", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "showDirectoryPicker", { configurable: true, value: undefined });
  });
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await page.goto("/");
  await page.getByTestId("open-collection").click();
  await expect(page.getByRole("alert")).toContainText("cannot open local folders");
  await expect(page.getByTestId("open-collection")).toBeEnabled();
  expect(errors).toEqual([]);
});

test("cancelling the folder picker leaves the app as it was", async ({ page }) => {
  await page.addInitScript(() => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => {
        throw new DOMException("The user aborted a request.", "AbortError");
      },
    });
  });
  await page.goto("/");
  await page.getByTestId("open-collection").click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByTestId("collection-name")).toHaveText("");
});

test("the real backend starts without loading the core", async ({ page }) => {
  await fakePicker(page);
  const wasmRequests: string[] = [];
  page.on("request", (r) => {
    if (r.url().includes("/wasm/")) wasmRequests.push(r.url());
  });
  await page.goto("/");
  await expect(page.getByTestId("open-collection")).toBeVisible();
  expect(await page.evaluate(() => (window as unknown as { __rt: { backend: { kind: string } } }).__rt.backend.kind)).toBe(
    "web",
  );
  expect(wasmRequests).toEqual([]);
});
