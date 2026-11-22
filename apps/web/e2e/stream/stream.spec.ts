// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 8, strand A (sessions UI): a WebSocket request created from the
// sidebar, its message variants, Connect / Send / Disconnect and the message history; an
// HTTP request read as Server-Sent Events. Mock backend only (scripted echo, see
// `apps/ui/src/commands/mock-streams.ts`); the real core runs in `e2e/shells/`.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { RequestSpec } from "@requesttailor/ui";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function createRequest(page: Page, name: string, kind: string) {
  await page.getByTestId("new-request").click();
  await page.getByTestId("new-request-kind").selectOption(kind);
  const field = page.getByTestId("new-request-name");
  await field.fill(name);
  await field.press("Enter");
  await expect(page.getByTestId("tab").filter({ hasText: name })).toHaveAttribute("aria-selected", "true");
}

const cm = (field: Locator) => field.locator(".cm-content");

async function setText(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  if (text === "") await content.press("Backspace");
  else await content.page().keyboard.type(text);
}

const backendSpec = (page: Page, path: string) =>
  page.evaluate((p) => (window as unknown as { __rt: { backend: { getRequest(p: string): Promise<RequestSpec> } } }).__rt.backend.getRequest(p), path);

test("a WebSocket request: messages, connect, echo, history, disconnect", async ({ page }) => {
  await openCollection(page);
  await createRequest(page, "Echo socket", "websocket");
  const created = page.getByTestId("tree-request").filter({ hasText: "Echo socket" });
  await expect(created).toContainText("WS");
  await setText(page.getByTestId("url-input"), "ws://127.0.0.1:47811/ws/echo");

  // Messages instead of Body; two variants, the second selected.
  await expect(page.getByTestId("tab-body")).toHaveCount(0);
  await page.getByTestId("tab-messages").click();
  await setText(page.getByTestId("message-data"), '{"hello":"world"}');
  await page.getByTestId("message-add").click();
  await expect(page.getByTestId("message-variant")).toHaveText(["Message 1", "Message 2"]);
  await setText(page.getByTestId("message-data"), "second");
  await expect
    .poll(async () => {
      const p = (await backendSpec(page, "echo-socket.yml")).protocol;
      return p?.type === "websocket" ? p.messages.map((m) => [m.title, m.selected, m.data]) : null;
    })
    .toEqual([
      ["Message 1", false, '{"hello":"world"}'],
      ["Message 2", true, "second"],
    ]);

  // The response panel is the session pane; Connect replaces Send.
  await expect(page.getByTestId("send")).toHaveCount(0);
  await expect(page.getByTestId("session-status")).toHaveText("Not connected");
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toHaveText("Connected");
  await expect(page.getByTestId("connect")).toHaveText("Disconnect");

  // Send the selected message, then the first one with the Send shortcut.
  await page.getByTestId("session-send").click();
  const rows = page.getByTestId("stream-row");
  await expect(rows.filter({ hasText: "second" })).toHaveCount(2);
  await page.getByTestId("message-variant").filter({ hasText: "Message 1" }).click();
  await page.getByTestId("stream-list").click();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(rows.filter({ hasText: '{"hello":"world"}' })).toHaveCount(2);
  await expect(page.getByTestId("stream-count")).toHaveText("4 messages");
  await expect(rows.locator('[aria-label="sent"]')).toHaveCount(2);
  await expect(rows.locator('[aria-label="received"]')).toHaveCount(2);

  // Filter, then open a message: pretty JSON.
  await page.getByTestId("stream-filter").fill("hello");
  await expect(page.getByTestId("stream-count")).toHaveText("2 of 5");
  await rows.filter({ hasText: "hello" }).last().click();
  await expect(page.getByTestId("stream-detail")).toContainText("Received");
  await expect(page.getByTestId("stream-detail-text")).toHaveText('{\n  "hello": "world"\n}');
  await page.getByTestId("stream-filter").fill("");

  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toHaveText("Closed by you (1000)");
  await expect(page.getByTestId("connect")).toContainText("Connect");
  await expect(page.getByTestId("session-send")).toBeDisabled();

  // Mod+Enter connects again (a fresh history).
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("session-status")).toHaveText("Connected");
  await expect(page.getByTestId("stream-count")).toHaveText("0 messages");
});

test("the server closes a WebSocket session", async ({ page }) => {
  await openCollection(page);
  await createRequest(page, "Closing socket", "websocket");
  await page.getByTestId("tab-messages").click();
  await setText(page.getByTestId("message-data"), "close");
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toHaveText("Connected");
  await page.getByTestId("session-send").click();
  await expect(page.getByTestId("session-status")).toHaveText("Closed by the server (1000 bye)");
});

test("an HTTP request with Accept: text/event-stream reads SSE", async ({ page }) => {
  await openCollection(page);
  await createRequest(page, "Ticks", "http");
  await page.getByTestId("tab-headers").click();
  const table = page.getByTestId("headers-table");
  await table.getByTestId("kv-add").click();
  const row = table.getByTestId("kv-row").nth(0);
  await setText(row.getByTestId("kv-name"), "Accept");
  await setText(row.getByTestId("kv-value"), "text/event-stream");

  await expect(page.getByTestId("connect")).toBeVisible();
  await expect(page.getByTestId("session-send")).toHaveCount(0);
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toHaveText("Closed by the server");
  const rows = page.getByTestId("stream-row");
  await expect(rows.filter({ hasText: "tick#" })).toHaveCount(3);
  await expect(rows.filter({ hasText: "tick#3" })).toContainText('{"tick":3}');
});

test("new gRPC and GraphQL requests", async ({ page }) => {
  await openCollection(page);
  await createRequest(page, "Say hello", "grpc");
  await expect(page.getByTestId("tab-headers")).toHaveText("Metadata");
  await page.getByTestId("tab-messages").click();
  await expect(page.getByTestId("grpc-method")).toBeVisible();
  await expect(page.getByTestId("message-kind")).toHaveValue("json");
  await expect(page.getByTestId("connect")).toBeVisible();

  await createRequest(page, "Query", "graphql");
  await expect(page.getByTestId("send")).toBeVisible();
  await expect(page.getByTestId("tab-headers")).toHaveText("Headers");
  expect((await backendSpec(page, "query.yml")).body).toEqual({ type: "graphql", query: "", variables: "" });
});
