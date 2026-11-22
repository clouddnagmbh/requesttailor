// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand A in the browser with the real core: the demo plugin
// (crates/rt-plugin/tests/fixtures/demo.wasm) runs in rt-wasm's plugin host. Installed via
// the dialog or the command layer, kept in IndexedDB across reloads, it adds a header and
// notes to sends against rt-testserver, renders its tabs and imports; disabled it does
// nothing, removed it is gone.

import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, test, type Page } from "@playwright/test";

import { fakePicker, fillFolder, sampleCollection } from "./env";

const DEMO = readFileSync(fileURLToPath(new URL("../../../../crates/rt-plugin/tests/fixtures/demo.wasm", import.meta.url)));

async function setup(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
}

/** Opens the folder through the UI (after a reload too) and waits until it is shown. */
async function open(page: Page): Promise<void> {
  const name = page.getByTestId("collection-name");
  // After a reload the app may already have reopened the folder by itself.
  if (!(await name.isVisible())) await page.getByTestId("open-collection").click();
  await expect(name).toHaveText("Shell test");
}

type Rt = { __rt: { backend: import("@requesttailor/ui").Backend } };

/** Sends health.yml with the Local environment; the echoed `x-demo-plugin` header and the notes. */
async function sendHealth(page: Page) {
  return page.evaluate(async () => {
    const b = (window as unknown as Rt).__rt.backend;
    const sent = await b.send("health.yml", { environment: "Local", vars: [] });
    const lines = await b.responseLines(sent.response_id!, 0, 1000);
    const echo = JSON.parse(lines.lines.join("\n")) as { headers: [string, string][] };
    return {
      status: sent.report.response?.status,
      header: echo.headers.find(([n]) => n.toLowerCase() === "x-demo-plugin")?.[1] ?? null,
      notes: (sent.report.notes ?? []).map((n) => `${n.source}: ${n.text}`),
      id: sent.response_id,
    };
  });
}

test("the demo plugin runs in the browser: send, tabs, import, reload, disable, remove", async ({ page }) => {
  await setup(page);
  const installed = await page.evaluate(async (bytes) => {
    const b = (window as unknown as Rt).__rt.backend;
    const file = { name: "demo.wasm", data: new Uint8Array(bytes) };
    const manifest = await b.plugins.inspect(file);
    const info = await b.plugins.install(file);
    let junk = "";
    try {
      await b.plugins.inspect({ name: "junk.wasm", data: new Uint8Array([1, 2, 3]) });
    } catch (e) {
      junk = `${(e as { kind?: string }).kind}`;
    }
    return { manifest, info, junk };
  }, [...DEMO]);
  expect(installed.manifest).toMatchObject({ id: "demo", version: "1.0.0", permissions: { storage: true } });
  expect(installed.info).toMatchObject({ granted: true, enabled: true });
  expect(installed.junk).toBe("plugin");

  await open(page);
  const sent = await sendHealth(page);
  expect(sent.status).toBe(200);
  expect(sent.header).toBe("1.0.0");
  expect(sent.notes).toEqual(["plugin:demo: Demo plugin added X-Demo-Plugin", "plugin:demo: Demo plugin saw the response"]);

  const tabs = await page.evaluate(async (id) => {
    const b = (window as unknown as Rt).__rt.backend;
    const req = await b.plugins.renderTab("demo", "demo-request", { request_path: "health.yml", response_id: null }, null);
    const res = await b.plugins.action("demo", "demo-response", "hello", { request_path: "health.yml", response_id: id }, null, []);
    const plan = await b.imports.analyzeWithPlugin([{ name: "a.demo", data: new Uint8Array([1]) }], "demo", "demo");
    await b.imports.discard(plan.id);
    return { req: req.view.blocks.length, res: res.view.blocks.map((x) => x.type), plan };
  }, sent.id);
  expect(tabs.req).toBe(4);
  expect(tabs.res).toEqual(["notice"]);
  expect(tabs.plan).toMatchObject({ format: "plugin", collection_name: "Demo import" });

  // IndexedDB keeps it across a reload.
  await page.reload();
  await open(page);
  expect((await sendHealth(page)).header).toBe("1.0.0");

  await page.evaluate(async () => (window as unknown as Rt).__rt.backend.plugins.setEnabled("demo", false));
  const off = await sendHealth(page);
  expect(off.header).toBeNull();
  expect(off.notes).toEqual([]);

  await page.reload();
  await open(page);
  expect((await sendHealth(page)).header).toBeNull();
  const after = await page.evaluate(async () => {
    const b = (window as unknown as Rt).__rt.backend;
    await b.plugins.remove("demo");
    return b.plugins.list();
  });
  expect(after).toEqual([]);
});

test("in the UI: install with confirmation, see the notes and the plugin tab", async ({ page }) => {
  await setup(page);
  await open(page);
  await page.getByTestId("env-switcher").selectOption("Local");

  await page.keyboard.press(`${process.platform === "darwin" ? "Meta" : "Control"}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await page.getByTestId("plugins-file").setInputFiles({ name: "demo.wasm", mimeType: "application/wasm", buffer: DEMO });
  await expect(page.getByTestId("plugins-permissions")).toHaveText("Storage: keep its own data on this device");
  await page.getByTestId("plugins-confirm-install").click();
  await expect(page.getByTestId("plugin-item")).toHaveCount(1);
  await page.getByTestId("plugins-close").click();

  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("send-note").first()).toContainText("Demo plugin added X-Demo-Plugin");
  await page.getByTestId("tab-plugin-demo/demo-request").click();
  await expect(page.getByTestId("request-editor").getByTestId("plugin-view")).toBeVisible();
});

// Wave 5 gate: the official example plugin (plugins/example, built by
// `node scripts/build-plugins.mjs`) runs unchanged in the browser, as on the desktop and in
// the CLI.
const EXAMPLE = fileURLToPath(new URL("../../../../target/plugins/example.wasm", import.meta.url));

test("the example plugin runs unchanged in the browser", async ({ page }) => {
  test.skip(!existsSync(EXAMPLE), "run `node scripts/build-plugins.mjs` first");
  await setup(page);
  await page.evaluate(async (bytes) => {
    const b = (window as unknown as Rt).__rt.backend;
    await b.plugins.install({ name: "example.wasm", data: new Uint8Array(bytes) });
  }, [...readFileSync(EXAMPLE)]);
  await open(page);
  const result = await page.evaluate(async () => {
    const b = (window as unknown as Rt).__rt.backend;
    const sent = await b.send("health.yml", { environment: "Local", vars: [] });
    const header = sent.report.request?.headers.find(([n]) => n === "X-Request-ID")?.[1] ?? "";
    const tab = await b.plugins.renderTab("example", "summary", { request_path: "health.yml", response_id: sent.response_id }, null);
    return {
      status: sent.report.response?.status,
      header,
      sources: (sent.report.notes ?? []).map((n) => n.source),
      blocks: tab.view.blocks.length,
    };
  });
  expect(result.status).toBe(200);
  expect(result.header).toMatch(/^rt-/);
  expect(result.sources).toEqual(["plugin:example", "plugin:example"]);
  expect(result.blocks).toBeGreaterThan(0);
});
