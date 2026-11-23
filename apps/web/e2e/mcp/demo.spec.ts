// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14: the MCP view explains itself and has a demo server (mock backend). First visit:
// what MCP is, "Try the demo server", "Connect to a server…", "Learn more" (opens only on
// click). The guided first run: demo connected, tools listed, fake_weather called with the
// form, saved as a test, "Run all" green. Resources, prompts and notifications of the demo;
// "Connect to RequestTailor itself" in the web app explains why it needs the desktop app.

import { expect, test, type Page } from "@playwright/test";

async function openMcp(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("view-mcp").click();
}

const step = (page: Page, id: string) => page.locator(`[data-testid="mcp-guide-step"][data-step="${id}"]`);
const tool = (page: Page, name: string) => page.locator(`[data-testid="mcp-tool"][data-name="${name}"]`);

test("first visit explains MCP; the guide goes from the demo server to a green Run all", async ({ page, context }) => {
  await openMcp(page);
  const sidebar = page.getByTestId("mcp-sidebar");
  await expect(sidebar).toContainText("MCP (Model Context Protocol) is how AI assistants call tools on servers.");
  await expect(sidebar).toContainText("rt mcp makes your collections available to AI agents");
  await expect(page.getByTestId("mcp-learn-more")).toHaveAttribute("href", /README\.md#mcp/);
  // Nothing is opened before a click.
  expect(context.pages()).toHaveLength(1);

  await page.getByTestId("mcp-try-demo").click();
  await expect(page.getByTestId("mcp-panel")).toBeVisible();
  await expect(page.getByTestId("mcp-server-title")).toHaveText("Demo server (built in)");
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "10");
  await expect(step(page, "connect")).toHaveAttribute("data-state", "done");
  await expect(step(page, "list")).toHaveAttribute("data-state", "done");
  await expect(step(page, "call")).toHaveAttribute("data-state", "next");
  await expect(page.getByTestId("mcp-coach")).toHaveAttribute("data-step", "call");
  await expect(tool(page, "fake_weather")).toHaveAttribute("aria-selected", "true");

  await page.getByTestId("mcp-arg-input").first().fill("Vienna");
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-text").first()).toContainText("Vienna: 21 °C, sunny");
  await expect(step(page, "save")).toHaveAttribute("data-state", "next");

  await page.getByTestId("mcp-save-test").click();
  await expect(page.getByTestId("mcp-assertion").first()).toBeVisible();
  await page.getByTestId("mcp-save-confirm").click();
  await expect(page.getByTestId("mcp-tests")).toHaveAttribute("data-count", "1");
  await expect(step(page, "run")).toHaveAttribute("data-state", "next");

  await page.getByTestId("mcp-run-all").click();
  await expect(page.getByTestId("mcp-test")).toHaveAttribute("data-state", "passed");
  await expect(page.getByTestId("mcp-tests-summary")).toContainText("1 passed");
  await expect(page.getByTestId("mcp-guide")).toHaveAttribute("data-next", "done");
  await expect(page.getByTestId("mcp-coach")).toBeHidden();

  // Finished on this device: the next visit shows no guide.
  await page.reload();
  await page.getByTestId("open-collection").click();
  await page.getByTestId("view-mcp").click();
  await page.getByTestId("mcp-try-demo").click();
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "10");
  await expect(page.getByTestId("mcp-guide")).toBeHidden();
});

test("the guide can be hidden, and comes back only on request", async ({ page }) => {
  await openMcp(page);
  await page.getByTestId("mcp-try-demo").click();
  await expect(page.getByTestId("mcp-guide")).toBeVisible();
  await page.getByTestId("mcp-guide-hide").click();
  await expect(page.getByTestId("mcp-guide")).toBeHidden();
  await expect(page.getByTestId("mcp-coach")).toBeHidden();
  await page.getByTestId("mcp-try-demo").click();
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "10");
  await expect(page.getByTestId("mcp-guide")).toBeHidden();

  // The explanation in the sidebar offers to start it again.
  await page.getByTestId("mcp-guide-show").click();
  await expect(page.getByTestId("mcp-guide")).toBeVisible();
  await expect(step(page, "call")).toHaveAttribute("data-state", "next");
});

test("the demo has resources, prompts and notifications", async ({ page }) => {
  await openMcp(page);
  await page.getByTestId("mcp-try-demo").click();
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "10");

  await tool(page, "countdown").click();
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-notification")).toHaveCount(6);
  await expect(page.getByTestId("mcp-result-notification").first()).toContainText("step 1 of 3");

  await page.getByRole("tab", { name: /Resources/ }).click();
  await expect(page.getByTestId("mcp-resources")).toHaveAttribute("data-count", "3");
  await page.locator('[data-testid="mcp-resource"][data-uri="demo://cities.json"]').click();
  await expect(page.getByTestId("mcp-resource-text")).toContainText("Vienna");

  await page.getByRole("tab", { name: /Prompts/ }).click();
  await page.locator('[data-testid="mcp-prompt"][data-name="summarize"]').click();
  await expect(page.getByTestId("mcp-prompt-get")).toBeDisabled();
  await page.locator('[data-testid="mcp-prompt-arg"][data-name="text"]').fill("MCP lets assistants call tools.");
  await page.getByTestId("mcp-prompt-get").click();
  await expect(page.getByTestId("mcp-prompt-messages")).toContainText("MCP lets assistants call tools.");
});

test("Connect to RequestTailor itself shows the collection as agents see it", async ({ page }) => {
  await openMcp(page);
  await page.getByTestId("mcp-try-self").click();
  await expect(page.getByTestId("mcp-self-hint")).toContainText("rt mcp");
  await expect(page.getByTestId("mcp-server-title")).toHaveText("RequestTailor itself (rt mcp)");
  await expect(tool(page, "list_requests")).toBeVisible();
});
