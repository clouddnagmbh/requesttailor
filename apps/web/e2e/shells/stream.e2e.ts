// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 8, strand A with the real core in the browser (rt-wasm `Workspace.openStream`) on an
// OPFS folder:
//   1. without the companion: SSE straight over `fetch`; a WebSocket session fails with a
//      message that says the companion is needed;
//   2. through a real `rt agent` (`GET /v1/stream`): a WebSocket echo with a secret in the
//      URL and the message (masked in the history), and SSE.
// The second part needs RT_BIN (the `rt` binary), e.g.
//   RT_BIN=$PWD/target/debug/rt RT_TESTSERVER_BIN=$PWD/target/debug/rt-testserver \
//     npm run e2e:shells -w apps/web
//
// The page's companion client talks to the default port 47812; an init script points those
// requests (fetch and WebSocket) at this test's agent, so a companion the user runs is never
// touched.

import { expect, test, type Page } from "@playwright/test";

import { startProcess } from "../git/server";
import { fakePicker, fillFolder, SERVER, SERVER_PORT, sampleCollection } from "./env";

/** Free for wave 8, strand A (47881 is the agent_stream Rust test). */
const AGENT_PORT = Number(process.env.RT_STREAM_AGENT_PORT ?? 47882);
const { RT_BIN } = process.env;
const SECRET = ["ws", "s3cret", "4711"].join("-");

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

const WS = `ws://127.0.0.1:${SERVER_PORT}`;

function collection(): Record<string, string> {
  return {
    ...sampleCollection(),
    "socket.yml": [
      "info:",
      "  name: Echo socket",
      "  type: websocket",
      "  seq: 3",
      "",
      "websocket:",
      `  url: "${WS}/ws/echo?t={{token}}"`,
      "  message:",
      "    type: text",
      '    data: "hello {{token}}"',
      "",
    ].join("\n"),
    "ticks.yml": [
      "info:",
      "  name: Ticks",
      "  type: http",
      "  seq: 4",
      "",
      "http:",
      "  method: GET",
      '  url: "{{baseUrl}}/sse/count/3"',
      "  headers:",
      "    - name: Accept",
      "      value: text/event-stream",
      "",
    ].join("\n"),
  };
}

async function open(page: Page, hash = "") {
  await fakePicker(page);
  await page.goto(`/${hash}`);
  await fillFolder(page, collection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.evaluate((secret) => (window as unknown as W).__rt.backend.secrets.set("Local", "token", secret), SECRET);
  await page.getByTestId("env-switcher").selectOption("Local");
}

async function select(page: Page, name: string) {
  await page.getByTestId("view-requests").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
}

async function sse(page: Page) {
  await select(page, "Ticks");
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toHaveText("Closed by the server");
  const rows = page.getByTestId("stream-row");
  await expect(page.getByTestId("stream-count")).toHaveText("3 messages");
  const received = rows.filter({ has: page.locator('[aria-label="received"]') });
  await expect(received).toHaveCount(3);
  await expect(received.nth(2)).toContainText("#3");
}

test("without the companion: SSE over fetch, WebSocket says the companion is needed", async ({ page }) => {
  await open(page);
  await sse(page);

  await select(page, "Echo socket");
  await page.getByTestId("connect").click();
  await expect(page.getByTestId("session-status")).toContainText("need the companion (`rt agent`)");
});

test.describe("through a real rt agent", () => {
  test.skip(!RT_BIN, "needs RT_BIN (the rt binary) for the companion's session relay");
  test.describe.configure({ mode: "serial" });

  let agent: ReturnType<typeof startProcess>;

  test.beforeAll(async ({}, info) => {
    const origin = new URL(String(info.project.use.baseURL)).origin;
    agent = startProcess(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
  });

  test.afterAll(() => {
    agent?.child.kill();
  });

  test("a WebSocket echo and SSE through the companion, secrets masked", async ({ page }) => {
    const code = (await agent.line(/#rt-pair=(\S+)/))[1];
    await page.addInitScript(
      ({ from, to }) => {
        const original = window.fetch.bind(window);
        window.fetch = (input: RequestInfo | URL, init?: RequestInit) =>
          original(typeof input === "string" && input.startsWith(from) ? to + input.slice(from.length) : input, init);
        const fromWs = from.replace(/^http/, "ws");
        const toWs = to.replace(/^http/, "ws");
        const Native = window.WebSocket;
        window.WebSocket = class extends Native {
          constructor(url: string | URL, protocols?: string | string[]) {
            const target = String(url);
            super(target.startsWith(fromWs) ? toWs + target.slice(fromWs.length) : target, protocols);
          }
        };
      },
      { from: "http://127.0.0.1:47812", to: `http://127.0.0.1:${AGENT_PORT}` },
    );
    // The pairing link pairs this page with the agent as it opens.
    await open(page, `#rt-pair=${code}`);
    await page.evaluate(() => (window as unknown as W).__rt.backend.transport.useCompanion(true));

    await select(page, "Echo socket");
    await page.getByTestId("connect").click();
    await expect(page.getByTestId("session-status")).toHaveText("Connected");
    await page.getByTestId("session-send").click();
    const rows = page.getByTestId("stream-row");
    await expect(rows.filter({ hasText: "hello ***" })).toHaveCount(2);
    await expect(rows.locator('[aria-label="received"]')).toHaveCount(1);
    await page.getByTestId("connect").click();
    await expect(page.getByTestId("session-status")).toHaveText("Closed by you (1000)");
    await expect(page.getByTestId("session-pane")).not.toContainText(SECRET);

    // The secret went out (the server echoed it), but nothing the page keeps shows it.
    const logs = await page.evaluate(async () => {
      const b = (window as unknown as W).__rt.backend;
      const seen: unknown[] = [];
      const id = await b.streams.open("socket.yml", { environment: "Local", vars: [] }, (e) => seen.push(e));
      await b.streams.send(id, { title: "", selected: true, kind: "text", data: "again {{token}}" });
      for (let i = 0; i < 100 && !seen.some((e) => (e as { direction?: string }).direction === "received"); i++) {
        await new Promise((r) => setTimeout(r, 20));
      }
      await b.streams.close(id);
      const log = await b.streams.log(id);
      await b.streams.release(id);
      return JSON.stringify({ seen, log });
    });
    expect(logs).toContain("again ***");
    expect(logs).toContain('"direction":"received"');
    expect(logs).not.toContain(SECRET);

    await sse(page);

    // Through the companion, natively: the agent logged the sessions' kind and host only.
    expect(agent.stderr()).toMatch(new RegExp(`WebSocket 127\\.0\\.0\\.1:${SERVER_PORT}`));
    expect(agent.stderr()).toMatch(new RegExp(`SSE 127\\.0\\.0\\.1:${SERVER_PORT}`));
    expect(agent.stderr()).not.toContain(SECRET);
    expect(SERVER).toContain(String(SERVER_PORT));
  });
});
