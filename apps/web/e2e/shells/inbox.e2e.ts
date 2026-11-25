// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand E with the real core: the webhook inbox of the web app runs in a real `rt agent`
// (`GET /v1/inbox`), and "Make request" is the core's (rt-wasm `inboxMakeRequest`) on an OPFS
// folder. Needs RT_BIN (the `rt` binary):
//   RT_BIN=$PWD/target/debug/rt RT_TESTSERVER_BIN=$PWD/target/debug/rt-testserver \
//     npm run e2e:shells -w apps/web
//
// The page's companion client talks to the default port 47812; an init script points those
// requests (fetch and WebSocket) at this test's agent, so a companion the user runs is never
// touched. The inbox of the test listens on `RT_INBOX_PORT` (default 48607).

import { expect, test, type Page } from "@playwright/test";

import { startProcess } from "../git/server";
import { fakePicker, fillFolder, sampleCollection } from "./env";

const AGENT_PORT = Number(process.env.RT_INBOX_AGENT_PORT ?? 48606);
const INBOX_PORT = Number(process.env.RT_INBOX_PORT ?? 48607);
const { RT_BIN } = process.env;
const CANARY = "RT-CANARY-inbox-shell-9b2e";

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

test.skip(!RT_BIN, "needs RT_BIN (the rt binary) for the companion's inbox");
test.describe.configure({ mode: "serial" });

let agent: ReturnType<typeof startProcess>;

test.beforeAll(async ({}, info) => {
  const origin = new URL(String(info.project.use.baseURL)).origin;
  agent = startProcess(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
});

test.afterAll(() => {
  agent?.child.kill();
});

async function open(page: Page, code: string) {
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
  await fakePicker(page);
  await page.goto(`/#rt-pair=${code}`);
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
}

test("receives a webhook on loopback, answers fixed and makes a request without secrets", async ({ page }) => {
  const code = (await agent.line(/#rt-pair=(\S+)/))[1];
  await open(page, code);

  // Nothing listens before the user starts it.
  await expect(fetch(`http://127.0.0.1:${INBOX_PORT}/`).then(() => "open", () => "closed")).resolves.toBe("closed");

  await page.evaluate(
    (port) => (window as unknown as W).__rt.backend.inbox.setSettings({ port, status: 202, body: "ok", content_type: "text/plain", delay_ms: 0 }),
    INBOX_PORT,
  );
  await page.keyboard.press("ControlOrMeta+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Inbox: Start");
  await palette.getByRole("option").filter({ hasText: "Inbox: Start" }).first().click();
  await expect(page.getByTestId("inbox-url")).toHaveText(`http://127.0.0.1:${INBOX_PORT}/`);

  // A service calls the inbox: fixed answer, no CORS freigabe.
  const answer = await fetch(`http://127.0.0.1:${INBOX_PORT}/hooks/orders?api_key=${CANARY}`, {
    method: "POST",
    headers: { "Content-Type": "application/json", Authorization: `Bearer ${CANARY}`, "X-Event": "order.created" },
    body: '{"order":42,"password":"' + CANARY + '"}',
  });
  expect(answer.status).toBe(202);
  expect(await answer.text()).toBe("ok");
  expect(answer.headers.get("access-control-allow-origin")).toBeNull();

  const rows = page.getByTestId("inbox-row");
  await expect(rows).toHaveCount(1);
  await expect(rows.first()).toContainText("/hooks/orders");
  await rows.first().click();
  await expect(page.getByTestId("inbox-headers")).toContainText("order.created");
  await expect(page.getByTestId("inbox-body-view")).toContainText('"order": 42');

  // Make request: the core's rules; secrets become variables without a value.
  await page.getByTestId("inbox-make-request").click();
  const dialog = page.getByTestId("additions-dialog");
  await expect(dialog.getByTestId("additions-requests")).toContainText("POST /hooks/orders");
  await expect(dialog.getByTestId("additions-environments")).toContainText("authorization (secret, set it yourself)");
  await expect(dialog).not.toContainText(CANARY);
  await dialog.getByRole("button", { name: /add|confirm|ok/i }).first().click();
  await expect(dialog).toHaveCount(0);

  const state = await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const tree = await b.tree();
    const envs = await b.environments();
    const draft = await b.inbox.makeRequest(1);
    return JSON.stringify({ tree, envs, draft });
  });
  expect(state).not.toContain(CANARY);
  expect(state).toContain("{{baseUrl}}/hooks/orders");
  const local = (JSON.parse(state) as { envs: { name: string; variables: { name: string; secret: boolean; value: string | null }[] }[] }).envs.find((e) => e.name === "Local")!;
  const auth = local.variables.find((v) => v.name === "authorization");
  expect(auth).toMatchObject({ secret: true, value: null });

  // It is memory only: the agent never wrote the capture anywhere it logs.
  expect(agent.stderr()).not.toContain(CANARY);

  // Stop frees the port.
  await page.getByTestId("inbox-toggle").click();
  await expect(page.getByTestId("inbox-off")).toBeVisible();
  await expect(fetch(`http://127.0.0.1:${INBOX_PORT}/`).then(() => "open", () => "closed")).resolves.toBe("closed");
});
