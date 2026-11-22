// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 8, strand D: the MCP client view (mock backend). Open it from the palette and the
// sidebar, connect, list tools, call with the schema form and raw JSON, see results, save a
// call as a test (staged, written by Save), run saved tests with pass/fail; stdio tests ask
// before they start a command.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

const tool = (page: Page, name: string) => page.locator(`[data-testid="mcp-tool"][data-name="${name}"]`);

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function connect(page: Page, url = "http://localhost:3333/mcp") {
  await page.getByTestId("mcp-url").fill(url);
  await page.getByTestId("mcp-connect").click();
}

test("connects from the palette, lists and calls tools, saves and runs a test", async ({ page }) => {
  await openCollection(page);
  await page.keyboard.press(`${MOD}+k`);
  await page.keyboard.type("Test an MCP");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("mcp-panel")).toBeVisible();
  await expect(page.getByTestId("view-mcp")).toHaveAttribute("aria-selected", "true");

  await connect(page, "http://unreachable/mcp");
  await expect(page.getByTestId("mcp-error")).toContainText("connection refused");
  await connect(page);
  await expect(page.getByTestId("mcp-server-info")).toContainText("demo-mcp");
  await expect(page.getByTestId("mcp-tools")).toHaveAttribute("data-count", "4");
  await expect(tool(page, "echo")).toContainText("Echo (echo)");
  await expect(tool(page, "echo")).toContainText("read-only");
  await expect(tool(page, "echo")).toContainText("Returns the text it was given.");

  // Form from the schema.
  await tool(page, "echo").click();
  await expect(page.getByTestId("mcp-args-missing")).toContainText("text");
  await page.getByTestId("mcp-arg-input").fill("hello");
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-text")).toHaveText("hello");
  await expect(page.getByTestId("mcp-result-state")).toHaveText("OK");

  // Raw JSON, structured output.
  await tool(page, "add").click();
  await page.getByTestId("mcp-args-raw").click();
  await page.getByTestId("mcp-args-json").fill('{"a": 2, "b": 3}');
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-structured")).toContainText('"sum": 5');

  // Tool errors and images.
  await tool(page, "fail").click();
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-state")).toHaveText("Tool error");
  await tool(page, "image").click();
  await page.getByTestId("mcp-call").click();
  await expect(page.getByTestId("mcp-result-image")).toBeVisible();

  // Save the add call as a test.
  await tool(page, "add").click();
  await page.getByTestId("mcp-args-raw").click();
  await page.getByTestId("mcp-args-json").fill('{"a": 2, "b": 3}');
  await page.getByTestId("mcp-call").click();
  await page.getByTestId("mcp-save-test").click();
  await expect(page.getByTestId("mcp-assertion")).toHaveCount(2);
  await page.getByTestId("mcp-test-name").fill("Add numbers");
  await page.getByTestId("mcp-save-confirm").click();
  const saved = page.getByTestId("mcp-test");
  await expect(saved).toHaveAttribute("data-path", "mcp/add-numbers.mcp.yaml");

  // Save writes it like a request edit.
  await page.keyboard.press(`${MOD}+s`);
  await expect
    .poll(() => page.evaluate(() => (window as unknown as { __rt: { backend: { files: Map<string, string> } } }).__rt.backend.files.has("mcp/add-numbers.mcp.yaml")))
    .toBe(true);

  await saved.getByTestId("mcp-test-run").click();
  await expect(saved).toHaveAttribute("data-state", "passed");
  await expect(page.getByTestId("mcp-tests-summary")).toContainText("1 passed");

  // Opening a request shows the editor again; the MCP view comes back from the sidebar.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").first().click();
  await expect(page.getByTestId("mcp-panel")).toBeHidden();
  await page.getByTestId("view-mcp").click();
  await saved.getByRole("button", { name: "add-numbers" }).click();
  await expect(page.getByTestId("mcp-panel")).toBeVisible();
  await expect(page.getByTestId("mcp-url")).toHaveValue("http://localhost:3333/mcp");
});

test("a failing saved test shows why; stdio tests ask before they run", async ({ page }) => {
  await openCollection(page);
  await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    await b.mcp.saveTest(null, {
      name: "Echo wrong",
      server: { transport: "http", url: "http://localhost/mcp", headers: [] },
      tool: "echo",
      arguments: { text: "hi" },
      assertions: [{ expression: "res.body", operator: "eq", value: '"bye"', description: null, enabled: true }],
    });
    await b.mcp.saveTest(null, {
      name: "Local server",
      server: { transport: "stdio", command: "demo-mcp", args: ["--quiet"], env: [], cwd: null },
      tool: "echo",
      arguments: { text: "hi" },
      assertions: [],
    });
  });
  await page.getByTestId("view-mcp").click();
  await expect(page.getByTestId("mcp-tests")).toHaveAttribute("data-count", "2");

  await page.getByTestId("mcp-run-all").click();
  const confirm = page.getByTestId("mcp-stdio-confirm");
  await expect(confirm).toContainText("demo-mcp --quiet");
  await page.getByTestId("mcp-stdio-allow").click();

  const wrong = page.locator('[data-testid="mcp-test"][data-path="mcp/echo-wrong.mcp.yaml"]');
  await expect(wrong).toHaveAttribute("data-state", "failed");
  await expect(wrong.getByTestId("mcp-test-failure")).toContainText("res.body eq");
  await expect(page.locator('[data-testid="mcp-test"][data-path="mcp/local-server.mcp.yaml"]')).toHaveAttribute("data-state", "passed");
  await expect(page.getByTestId("mcp-tests-summary")).toContainText("1 failed");

  // Asked once per collection: the next run starts right away.
  await page.getByTestId("mcp-run-all").click();
  await expect(confirm).toBeHidden();
});
