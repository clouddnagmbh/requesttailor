// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The real companion client (`apps/web/src/companion/client.ts`) against a real `rt agent`
// in real Chrome: status, pairing by code and by pairing link, and `/v1/send` with the same
// request CompanionTransport makes, proxied to rt-testserver. This is what proves CORS
// (allowlisted origin, `Access-Control-Expose-Headers: x-rt-head`) in the browser.
//
// Needs the binaries, e.g.:
//   cargo build -p rt-cli -p rt-testserver
//   RT_BIN=target/debug/rt RT_TESTSERVER_BIN=target/debug/rt-testserver \
//     npx playwright test --config e2e/companion/playwright.companion.config.ts
// Skipped when RT_BIN or RT_TESTSERVER_BIN is not set.

import { spawn, type ChildProcess } from "node:child_process";
import { createInterface } from "node:readline";

import { expect, test } from "@playwright/test";

/** Companion test port (the default 47812 stays free for a real `rt agent`). */
const AGENT_PORT = 47822;
const AGENT_URL = `http://127.0.0.1:${AGENT_PORT}`;
const { RT_BIN, RT_TESTSERVER_BIN } = process.env;
/** The client module as the Vite dev server serves it to the page. */
type Client = typeof import("../../src/companion/client");

test.skip(!RT_BIN || !RT_TESTSERVER_BIN, "set RT_BIN and RT_TESTSERVER_BIN to run against a real rt agent");
test.describe.configure({ mode: "serial" });

/** Starts a process and resolves each stdout line that matches, in order. */
function start(bin: string, args: string[]) {
  const child = spawn(bin, args, { stdio: ["ignore", "pipe", "pipe"] });
  const waiting: { re: RegExp; resolve: (m: RegExpMatchArray) => void }[] = [];
  const seen: string[] = [];
  let stderr = "";
  child.stderr!.on("data", (d) => (stderr += String(d)));
  createInterface({ input: child.stdout! }).on("line", (line) => {
    const i = waiting.findIndex((w) => w.re.test(line));
    if (i < 0) {
      seen.push(line);
      return;
    }
    const [w] = waiting.splice(i, 1);
    w.resolve(line.match(w.re)!);
  });
  return {
    child,
    stderr: () => stderr,
    /** The next line matching `re` (also one already printed and not taken yet). */
    line(re: RegExp): Promise<RegExpMatchArray> {
      const i = seen.findIndex((l) => re.test(l));
      if (i >= 0) return Promise.resolve(seen.splice(i, 1)[0].match(re)!);
      return new Promise((resolve, reject) => {
        waiting.push({ re, resolve });
        child.once("exit", (code) => reject(new Error(`${bin} exited (${code}): ${stderr}`)));
      });
    },
  };
}

let agent: ReturnType<typeof start>;
let server: ChildProcess;
let target: string;

test.beforeAll(async ({}, info) => {
  const origin = new URL(String(info.project.use.baseURL)).origin;
  const testserver = start(RT_TESTSERVER_BIN!, ["--port", "0"]);
  server = testserver.child;
  target = (await testserver.line(/listening on (http:\/\/\S+)/))[1];
  agent = start(RT_BIN!, ["agent", "--port", String(AGENT_PORT), "--allow-origin", origin]);
});

test.afterAll(() => {
  agent?.child.kill();
  server?.kill();
});

const nextCode = async () => (await agent.line(/^Pairing code: (\S+)/))[1];

test("pairs by code and sends through the companion", async ({ page }) => {
  // The first request compiles client.ts in the dev server and the debug agent starts cold:
  // seconds on a shared runner (it passed on one run and hit 30 s on the next).
  test.setTimeout(90_000);
  const code = await nextCode();
  await page.goto("/?backend=mock");
  const result = await page.evaluate(
    async ({ url, code, target }) => {
      const { createCompanionClient }: Client = await import(/* @vite-ignore */ "/src/companion/client.ts" as string);
      const client = createCompanionClient(url);
      await client.forget();
      const before = await client.status();
      let wrong = "";
      await client.pair("0000-0000").catch((e: Error) => (wrong = e.message));
      await client.pair(code);
      const after = await client.status();
      const credentials = await client.credentials();
      // What CompanionTransport sends (rt-core, wasm32).
      const res = await fetch(`${credentials!.url}/v1/send`, {
        method: "POST",
        headers: { Authorization: `Bearer ${credentials!.token}`, "Content-Type": "application/json" },
        body: JSON.stringify({
          method: "POST",
          url: `${target}/echo/companion?q=1`,
          headers: [["X-Custom", "yes"]],
          body: btoa("hello companion"),
          content_type: "text/plain",
          timeout_ms: 10000,
          follow_redirects: true,
          max_redirects: 5,
        }),
      });
      const head = JSON.parse(res.headers.get("x-rt-head") ?? "null");
      const echo = await res.json();
      // A token that is not valid any more.
      const bad = await fetch(`${url}/v1/send`, {
        method: "POST",
        headers: { Authorization: "Bearer nope", "Content-Type": "application/json" },
        body: "{}",
      });
      return { before, wrong, after, status: res.status, head, echo, badStatus: bad.status };
    },
    { url: AGENT_URL, code, target },
  );
  expect(result.before).toEqual({
    available: true,
    paired: false,
    url: AGENT_URL,
    howTo: expect.stringContaining("rt agent"),
  });
  expect(result.wrong).toContain("wrong code");
  expect(result.after.paired).toBe(true);
  expect(result.status).toBe(200);
  expect(result.head.status).toBe(200);
  expect(result.head.url).toBe(`${target}/echo/companion?q=1`);
  expect(result.echo.method).toBe("POST");
  expect(result.echo.body).toBe("hello companion");
  expect(result.echo.headers).toContainEqual(["x-custom", "yes"]);
  // The companion sent it natively: no browser Origin or Sec-Fetch headers reach the target.
  expect(result.echo.headers.map((h: [string, string]) => h[0])).not.toContain("origin");
  expect(result.badStatus).toBe(401);
  expect(agent.stderr()).toMatch(/POST 127\.0\.0\.1:\d+ -> 200/);
});

test("pairs from the pairing link and removes the code from the address bar", async ({ page }) => {
  const code = await nextCode();
  await page.goto(`/?backend=mock#rt-pair=${code.replace("-", "")}`);
  const result = await page.evaluate(async (url) => {
    const { createCompanionClient }: Client = await import(/* @vite-ignore */ "/src/companion/client.ts" as string);
    localStorage.clear();
    const client = createCompanionClient(url);
    const status = await client.status();
    return { status, hash: location.hash, credentials: await client.credentials() };
  }, AGENT_URL);
  expect(result.status.paired).toBe(true);
  expect(result.hash).toBe("");
  expect(result.credentials?.token).toHaveLength(43);
});

test("forgets a token the companion no longer knows", async ({ page }) => {
  await page.goto("/?backend=mock");
  const result = await page.evaluate(async (url) => {
    const { createCompanionClient }: Client = await import(/* @vite-ignore */ "/src/companion/client.ts" as string);
    localStorage.setItem(`requesttailor.companion.token:${url}`, "stale-token");
    const client = createCompanionClient(url);
    const status = await client.status();
    return { status, credentials: await client.credentials() };
  }, AGENT_URL);
  expect(result.status).toMatchObject({ available: true, paired: false });
  expect(result.credentials).toBeNull();
});

test("a page from another origin cannot use the companion", async ({ page, baseURL }) => {
  // Same dev server, other origin (localhost instead of 127.0.0.1): not on the allowlist.
  const other = baseURL!.replace("127.0.0.1", "localhost");
  await page.goto(`${other}/?backend=mock`);
  const result = await page.evaluate(async (url) => {
    const { createCompanionClient }: Client = await import(/* @vite-ignore */ "/src/companion/client.ts" as string);
    const client = createCompanionClient(url);
    let pairError = "";
    await client.pair("ABCD-EFGH").catch((e: Error) => (pairError = e.message));
    return { status: await client.status(), pairError };
  }, AGENT_URL);
  // The companion answers 403 without CORS headers, so the browser hides even that.
  expect(result.status.available).toBe(false);
  // The hint names the origin unless it is one of rt agent's defaults (true on port 5173,
  // where this test agent was nevertheless started with a narrower allowlist).
  const otherOrigin = new URL(other).origin;
  const defaults = ["http://127.0.0.1:5173", "http://localhost:5173"];
  expect(result.status.howTo).toContain("rt agent");
  if (!defaults.includes(otherOrigin)) {
    expect(result.status.howTo).toContain(`--allow-origin ${otherOrigin}`);
  }
  expect(result.pairError).toContain("not reachable");
  expect(agent.stderr()).toContain("not allowed (see --allow-origin)");
});
