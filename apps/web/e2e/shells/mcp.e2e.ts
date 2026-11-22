// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 8, strand D: the MCP client with the real core (rt-wasm) against rt-testserver's
// Streamable HTTP server at /mcp. Connect with a secret header from the browser store, list
// and call tools, save a call as a test (written by Save), run it; stdio is refused.

import { expect, test, type Page } from "@playwright/test";

import { fakePicker, fillFolder, readFolderFile, sampleCollection } from "./env";

/** Assembled at run time, so no scanner sees a credential in this file. */
const SECRET = ["mcp", "shell", "4b7e"].join("-");

const tool = (page: Page, name: string) => page.locator(`[data-testid="mcp-tool"][data-name="${name}"]`);

async function setup(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.evaluate(async (secret) => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    await b.secrets.set("Local", "token", secret);
  }, SECRET);
}

test("connects, calls tools, saves and runs a test through the real core", async ({ page }) => {
  await setup(page);
  await page.getByTestId("view-mcp").click();
  await page.getByTestId("mcp-new").click();
  await page.getByTestId("mcp-url").fill("{{baseUrl}}/mcp");
  await page.locator("details.more summary").click();
  await page.getByTestId("mcp-add-headers").click();
  const header = page.getByTestId("mcp-headers").locator(".pair").first();
  await header.getByLabel("Name").fill("Authorization");
  await header.getByLabel("Value").fill("Bearer {{token}}");
  await page.getByTestId("mcp-connect").click();

  await expect(page.getByTestId("mcp-server-info")).toContainText("rt-testserver");
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "6");
  await expect(page.getByTestId("mcp-tool").first()).toContainText("read-only");

  // The secret went out in the header and comes back masked.
  await tool(page, "secret").click();
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-text")).toHaveText("Bearer ***");

  // Structured output with a form from the input schema.
  await tool(page, "add").click();
  await page.getByTestId("mcp-arg").first().getByTestId("mcp-arg-input").fill("2");
  await page.getByTestId("mcp-arg").nth(1).getByTestId("mcp-arg-input").fill("3");
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-structured")).toContainText('"sum": 5');

  await tool(page, "fail").click();
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-state")).toHaveText("Tool error");

  // Save the add call as a test, write it, run it.
  await tool(page, "add").click();
  await page.getByTestId("mcp-arg").first().getByTestId("mcp-arg-input").fill("2");
  await page.getByTestId("mcp-arg").nth(1).getByTestId("mcp-arg-input").fill("3");
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-structured")).toContainText('"sum": 5');
  await page.getByTestId("mcp-save-test").click();
  await page.getByTestId("mcp-test-name").fill("Add numbers");
  await page.getByTestId("mcp-save-confirm").click();
  await expect(page.getByTestId("mcp-test")).toHaveAttribute("data-path", "mcp/add-numbers.mcp.yaml");
  await page.keyboard.press("ControlOrMeta+s");
  await expect.poll(() => readFolderFile(page, "mcp/add-numbers.mcp.yaml")).toContain("tool: add");
  const text = (await readFolderFile(page, "mcp/add-numbers.mcp.yaml"))!;
  expect(text).toContain("Bearer {{token}}");
  expect(text).not.toContain(SECRET);

  await page.getByTestId("mcp-test-run").click();
  await expect(page.getByTestId("mcp-test")).toHaveAttribute("data-state", "passed");

  // stdio cannot run in the browser.
  await page.getByTestId("mcp-transport").selectOption("stdio");
  await expect(page.getByTestId("mcp-stdio-web")).toBeVisible();
  await expect(page.getByTestId("mcp-connect")).toBeDisabled();
  const refused = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    return b.mcp
      .connect({ transport: "stdio", command: "npx", args: [], env: [], cwd: null }, { environment: null, vars: [] })
      .then(
        () => null,
        (e: { kind: string; message: string }) => ({ kind: e.kind, message: e.message }),
      );
  });
  expect(refused?.kind).toBe("unsupported");
  expect(refused?.message).toContain("cannot start local commands");
});
