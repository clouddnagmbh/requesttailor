// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand B with the real core in the browser (rt-wasm) and a real `rt agent`: an
// extension (`?extensions=egress`) calls a host that sends no CORS headers. The first call asks
// once; `fetch` is refused by the browser and the call is repeated over the paired companion;
// the secret header reaches the target and is masked in what the extension gets back (canary);
// a redirect comes back as it is; a host the user never allowed is never contacted; the list of
// hosts survives a reload and Remove takes a host back; the call is in no history.
// Needs RT_BIN (the `rt` binary), e.g.
//   cargo build -p rt-cli && node scripts/build-wasm.mjs &&
//     RT_BIN=$PWD/target/debug/rt npx playwright test --config e2e/egress/playwright.egress.config.ts

import { createServer, type IncomingMessage, type Server } from "node:http";

import { expect, test, type Page } from "@playwright/test";

import { startProcess } from "../git/server";
import { fakePicker, fillFolder, sampleCollection } from "../shells/env";

const TARGET_PORT = Number(process.env.RT_EGRESS_TARGET_PORT ?? 48706);
const AGENT_PORT = Number(process.env.RT_EGRESS_AGENT_PORT ?? 48707);
const { RT_BIN } = process.env;
const CANARY = ["trace", "secret", "e2e", "5a1d"].join("-");
const MOD = "ControlOrMeta";

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };
type Result = { ok: true; value: { status: number; body: string; headers: [string, string][] } } | { ok: false; kind: string; message: string };

interface Hit {
  method: string;
  path: string;
  headers: IncomingMessage["headers"];
}

test.skip(!RT_BIN, "needs RT_BIN (the rt binary) for the companion path");
test.describe.configure({ mode: "serial" });

let agent: ReturnType<typeof startProcess>;
let target: Server;
const hits: Hit[] = [];

test.beforeAll(async ({}, info) => {
  const origin = new URL(String(info.project.use.baseURL)).origin;
  // The target sends no CORS headers at all: the browser refuses every call to it.
  target = createServer((req, res) => {
    hits.push({ method: req.method ?? "", path: req.url ?? "", headers: req.headers });
    if (req.url?.startsWith("/redirect")) {
      res.writeHead(302, { Location: "/final" });
      res.end();
      return;
    }
    const auth = String(req.headers.authorization ?? "");
    res.writeHead(200, { "content-type": "application/json", "x-echo-auth": auth });
    res.end(JSON.stringify({ spans: [{ name: "GET /orders", ms: 12 }], echoed: auth }));
  });
  await new Promise<void>((resolve) => target.listen(TARGET_PORT, "127.0.0.1", resolve));
  agent = startProcess(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
});

test.afterAll(async () => {
  agent?.child.kill();
  await new Promise<void>((resolve) => target?.close(() => resolve()));
});

const URL_OF = (path: string) => `http://127.0.0.1:${TARGET_PORT}${path}`;
const getHits = () => hits.filter((h) => h.method === "GET");

async function open(page: Page, hash = "") {
  await fakePicker(page);
  await page.goto(`/?extensions=egress${hash}`);
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
}

async function call(page: Page, path: string, headers: [string, string][] = []) {
  await page.evaluate(
    ({ url, headers }) => {
      const w = window as unknown as { __egressRequest: unknown; __egressResult?: unknown };
      w.__egressResult = undefined;
      w.__egressRequest = { method: "GET", url, headers, extension: "Trace viewer", purpose: "Server traces", environment: "Local" };
    },
    { url: URL_OF(path), headers },
  );
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Egress demo");
  await palette.getByRole("option").filter({ hasText: "Egress demo: send call" }).click();
}

const result = (page: Page) =>
  page.evaluate(async () => {
    const w = window as unknown as { __egressResult?: unknown };
    for (let i = 0; i < 300 && !w.__egressResult; i++) await new Promise((r) => setTimeout(r, 20));
    return w.__egressResult;
  }) as Promise<Result>;

test("ask once, the browser is refused, the companion carries the call, the secret stays masked", async ({ page }) => {
  const code = (await agent.line(/#rt-pair=(\S+)/))[1];
  // The page's companion client talks to the default port; point it at this test's agent.
  await page.addInitScript(
    ({ from, to }) => {
      const original = window.fetch.bind(window);
      window.fetch = (input: RequestInfo | URL, init?: RequestInit) =>
        original(typeof input === "string" && input.startsWith(from) ? to + input.slice(from.length) : input, init);
    },
    { from: "http://127.0.0.1:47812", to: `http://127.0.0.1:${AGENT_PORT}` },
  );
  await open(page, `#rt-pair=${code}`);
  // Paired, but not switched on: only a call the browser refuses uses the companion.
  await page.evaluate(async (value) => {
    const b = (window as unknown as W).__rt.backend;
    await b.transport.useCompanion(true);
    await b.transport.useCompanion(false);
    await b.secrets.set("Local", "token", value);
  }, CANARY);

  await call(page, "/api/traces/4bf9", [["Authorization", "Bearer {{secret:token}}"]]);
  await expect(page.getByTestId("egress-ask-host")).toHaveText(`127.0.0.1:${TARGET_PORT}`);
  await expect(page.getByTestId("egress-ask-secrets")).toContainText("token");
  await page.getByTestId("egress-allow").click();
  const answer = await result(page);
  expect(answer.ok, JSON.stringify(answer)).toBe(true);
  if (answer.ok) {
    expect(answer.value.status).toBe(200);
    expect(answer.value.body).toContain("spans");
  }
  // The secret reached the target (once, from the agent), and nowhere else: not in the answer,
  // not in the page's storage, not in the agent's log. The marker header goes no further.
  expect(getHits()).toHaveLength(1);
  expect(getHits()[0].headers.authorization).toBe(`Bearer ${CANARY}`);
  expect(getHits()[0].headers["x-rt-egress"]).toBeUndefined();
  expect(JSON.stringify(answer)).not.toContain(CANARY);
  const stored = await page.evaluate(() => JSON.stringify({ ...localStorage }));
  expect(stored).not.toContain(CANARY);
  expect(agent.stderr()).not.toContain(CANARY);
  expect(agent.stderr()).toMatch(new RegExp(`GET 127\\.0\\.0\\.1:${TARGET_PORT} -> 200`));

  // Nothing in the history.
  const history = await page.evaluate(() => (window as unknown as W).__rt.backend.history.list("", 20));
  expect(history).toEqual([]);

  // Remembered: the second call does not ask. A redirect comes back as it is, not followed.
  await call(page, "/redirect");
  const redirected = await result(page);
  expect(redirected.ok).toBe(true);
  if (redirected.ok) expect(redirected.value.status).toBe(302);
  await expect(page.getByTestId("egress-ask")).toHaveCount(0);
  expect(hits.some((h) => h.path === "/final")).toBe(false);
});

test("the list of hosts survives a reload; Remove takes the host back and nothing is contacted", async ({ page }) => {
  await page.goto("/?extensions=egress");
  await page.waitForFunction(() => Boolean((window as unknown as W).__rt));
  await page.evaluate(
    (port) => (window as unknown as W).__rt.backend.egress.allow("127.0.0.1", port, "Trace viewer", "Server traces"),
    TARGET_PORT,
  );
  await page.reload();
  await page.waitForFunction(() => Boolean((window as unknown as W).__rt));
  const kept = await page.evaluate(() => (window as unknown as W).__rt.backend.egress.hosts());
  expect(kept).toMatchObject([{ host: "127.0.0.1", port: TARGET_PORT, extension: "Trace viewer", purpose: "Server traces" }]);

  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await palette.getByRole("combobox").fill("Egress hosts");
  await palette.getByRole("option").filter({ hasText: "Egress hosts" }).click();
  const row = page.getByTestId("egress-host");
  await expect(row).toHaveCount(1);
  await expect(row).toHaveAttribute("data-host", `127.0.0.1:${TARGET_PORT}`);
  await expect(row).toContainText("Trace viewer · Server traces");
  await page.getByTestId("egress-remove").click();
  await expect(page.getByTestId("egress-host")).toHaveCount(0);
  await page.getByTestId("egress-hosts-done").click();

  const before = hits.length;
  await call(page, "/api/traces/again");
  await expect(page.getByTestId("egress-ask")).toBeVisible();
  await page.getByTestId("egress-deny").click();
  expect(await result(page)).toMatchObject({ ok: false, kind: "not-allowed" });
  expect(hits.length).toBe(before);
});
