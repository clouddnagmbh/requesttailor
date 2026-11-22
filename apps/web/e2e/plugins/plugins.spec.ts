// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 5, strand A: the plugin UI against the mock backend. Install with permission
// confirmation, update with re-confirmation, plugin tabs in the request and the response,
// send notes, the plugin importer in the import dialog, disable and remove. The mock takes
// the manifest as JSON in place of a `.wasm` file (apps/ui/src/commands/mock-plugins.ts).

import { expect, test, type Page } from "@playwright/test";

const MOD = process.platform === "darwin" ? "Meta" : "Control";

function manifest(version: string, network: string[]) {
  return {
    id: "demo",
    name: "Demo",
    version,
    api: 1,
    description: "A plugin for the UI tests.",
    license: "Apache-2.0",
    homepage: null,
    permissions: { network, storage: true, variables: false },
    contributes: {
      prepare: true,
      response: true,
      tabs: [
        { id: "req", title: "Demo request", place: "request", content_types: [] },
        { id: "json", title: "Demo JSON", place: "response", content_types: ["application/json"] },
        { id: "xml", title: "Demo XML", place: "response", content_types: ["application/xml"] },
      ],
      importers: [{ id: "imp", label: "Demo files", extensions: [".demo"] }],
    },
  };
}

async function openPlugins(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("plugins-dialog")).toBeVisible();
}

async function pick(page: Page, version: string, network: string[]) {
  await page.getByTestId("plugins-file").setInputFiles({
    name: "demo.wasm",
    mimeType: "application/wasm",
    buffer: Buffer.from(JSON.stringify(manifest(version, network))),
  });
}

test("install, use, update, disable and remove a plugin", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();

  // Install: the permissions are shown and confirmed first.
  await openPlugins(page);
  await expect(page.getByTestId("plugins-empty")).toBeVisible();
  await pick(page, "1.0.0", []);
  await expect(page.getByTestId("plugins-confirm")).toContainText("Install Demo 1.0.0?");
  await expect(page.getByTestId("plugins-permissions")).toHaveText("Storage: keep its own data on this device");
  await page.getByTestId("plugins-confirm-cancel").click();
  await expect(page.getByTestId("plugins-empty")).toBeVisible();
  await pick(page, "1.0.0", []);
  await page.getByTestId("plugins-confirm-install").click();
  await expect(page.getByTestId("plugin-item")).toHaveCount(1);
  await expect(page.getByTestId("plugin-version")).toHaveText("1.0.0");

  // Not a plugin: an error, nothing installed.
  await page.getByTestId("plugins-file").setInputFiles({ name: "junk.wasm", mimeType: "application/wasm", buffer: Buffer.from([0, 1]) });
  await expect(page.getByTestId("plugins-error")).toContainText("not a RequestTailor plugin");
  await page.getByTestId("plugins-close").click();

  // Request tab: renders the view; an action updates the request (unsaved).
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.getByTestId("tab-plugin-demo/req").click();
  const tab = page.getByTestId("plugin-tab");
  await expect(tab.getByRole("heading", { name: "Demo: req" })).toBeVisible();
  await tab.getByRole("button", { name: "Say hello" }).click();
  await expect(tab.getByTestId("plugin-notice")).toHaveText("Hello from Demo");
  await tab.getByLabel("Query").fill("abc");
  await tab.getByRole("button", { name: "Run" }).click();
  await expect(tab.getByTestId("plugin-notice")).toHaveText("Form: query=abc, mode=fast, verbose=false");
  await tab.getByRole("button", { name: "leaf" }).click();
  await expect(tab.getByTestId("plugin-notice")).toHaveText("Clicked leaf");
  await tab.getByRole("button", { name: "Add header" }).click();
  await expect(tab.getByTestId("plugin-notice")).toHaveText("Header added");
  await page.getByTestId("tab-headers").click();
  await expect(page.getByTestId("headers-table")).toContainText("X-Mock-Plugin");

  // Send: the notes of the plugin hooks; the JSON response tab is offered, the XML one not.
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("send-note")).toHaveText([
    /Plugin demo\s*Demo prepared the request/,
    /Plugin demo\s*Demo saw the response/,
  ]);
  await expect(page.getByTestId("response-tab-plugin-demo/xml")).toHaveCount(0);
  await page.getByTestId("response-tab-plugin-demo/json").click();
  await expect(page.getByTestId("response-view").getByTestId("plugin-view")).toContainText(/Request users\/create-user.yml, response \d+/);

  // Update with more permissions: confirmed again, with what is new.
  await openPlugins(page);
  await pick(page, "2.0.0", ["{request}"]);
  await expect(page.getByTestId("plugins-confirm")).toContainText("Update Demo from 1.0.0 to 2.0.0?");
  await expect(page.getByTestId("plugins-permissions-added")).toHaveText("Network: the host of the request being sent");
  await page.getByTestId("plugins-confirm-install").click();
  await expect(page.getByTestId("plugin-version")).toHaveText("2.0.0");
  // The same version again asks for nothing new.
  await pick(page, "2.0.0", ["{request}"]);
  await expect(page.getByTestId("plugins-permissions-same")).toBeVisible();
  await page.getByTestId("plugins-confirm-cancel").click();

  // Disable: tabs and notes are gone.
  await page.getByTestId("plugin-enabled").uncheck();
  await expect(page.getByTestId("plugin-enabled")).not.toBeChecked();
  await page.getByTestId("plugins-close").click();
  await expect(page.getByTestId("tab-plugin-demo/req")).toHaveCount(0);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("send-note")).toHaveCount(0);

  // Remove (with a second click to confirm).
  await openPlugins(page);
  await page.getByTestId("plugin-remove").click();
  await page.getByTestId("plugin-remove-confirm").click();
  await expect(page.getByTestId("plugins-empty")).toBeVisible();
});

test("the import dialog offers plugin importers", async ({ page }) => {
  await page.goto("/?backend=mock");
  await openPlugins(page);
  await pick(page, "1.0.0", []);
  await page.getByTestId("plugins-confirm-install").click();
  await page.getByTestId("plugins-close").click();

  await page.getByTestId("import").click();
  await page.getByTestId("import-format").selectOption({ label: "Demo files (plugin Demo)" });
  await page.getByTestId("import-file").setInputFiles({ name: "a.demo", mimeType: "text/plain", buffer: Buffer.from("x") });
  await expect(page.getByTestId("import-detected")).toHaveText("Plugin");
  await expect(page.getByTestId("import-plan")).toContainText("Demo import");
});
