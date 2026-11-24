// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 17, strand E: every `code` block of a plugin view has a Copy button (what "Copy as ABAP" of
// the OData plugin relies on). Mock backend: the demo plugin's request tab renders a code block.

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

const manifest = {
  id: "demo",
  name: "Demo",
  version: "1.0.0",
  api: 1,
  description: "A plugin for the UI tests.",
  license: "Apache-2.0",
  homepage: null,
  permissions: { network: [], storage: true, variables: false },
  contributes: {
    prepare: false,
    response: false,
    tabs: [{ id: "req", title: "Demo request", place: "request", content_types: [] }],
    importers: [],
  },
};

async function install(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await page.getByTestId("plugins-file").setInputFiles({
    name: "demo.wasm",
    mimeType: "application/wasm",
    buffer: Buffer.from(JSON.stringify(manifest)),
  });
  await page.getByTestId("plugins-confirm-install").click();
  await expect(page.getByTestId("plugin-item")).toHaveCount(1);
  await page.getByTestId("plugins-close").click();
}

test("a code block of a plugin view can be copied", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await install(page);

  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.getByTestId("tab-plugin-demo/req").click();
  const view = page.getByTestId("plugin-tab");
  const code = view.getByTestId("plugin-code");
  await expect(code).toHaveText('{"mock": true}');

  const copy = view.getByTestId("plugin-code-copy");
  await expect(copy).toHaveText("Copy");
  await copy.click();
  await expect(copy).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe('{"mock": true}');
  // The label goes back by itself.
  await expect(copy).toHaveText("Copy");
});
