// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand A: comparing environments with the real core (rt-wasm) in the browser. Two
// rt-testserver instances stand for two environments (the shells' own server on
// `RT_SHELLS_SERVER_PORT`, a second one on `RT_SHELLS_SERVER2_PORT`, default 48583). Each
// environment has its own secret, a canary that must appear nowhere on the page.

import { spawn, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, readFolderFile } from "./env";

const SERVER2_PORT = Number(process.env.RT_SHELLS_SERVER2_PORT ?? 48583);
const SERVER2 = `http://127.0.0.1:${SERVER2_PORT}`;
const CANARY_DEV = "RT-CANARY-dev-3c9a1e";
const CANARY_QA = "RT-CANARY-qa-77d2b0";
const MOD = "ControlOrMeta";

type W = Window & { __rt: { backend: import("@requesttailor/ui").Backend } };

const request = (name: string, seq: number, method: string, url: string) =>
  ["info:", `  name: ${name}`, "  type: http", `  seq: ${seq}`, "", "http:", `  method: ${method}`, `  url: "${url}"`, ""].join("\n");

const env = (name: string, base: string) =>
  `name: ${name}\nvariables:\n  - name: baseUrl\n    value: ${base}\n  - secret: true\n    name: token\n`;

const files = (): Record<string, string> => ({
  "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Env compare\n',
  "environments/Dev.yml": env("Dev", SERVER),
  "environments/QA.yml": env("QA", SERVER2),
  "echo.yml": request("Echo", 1, "GET", "{{baseUrl}}/echo/item?t={{token}}"),
  "status.yml": request("Status", 2, "GET", "{{baseUrl}}/status/200"),
  "create.yml": request("Create", 3, "POST", "{{baseUrl}}/echo/create"),
});

let server2: ChildProcess | null = null;

test.beforeAll(async () => {
  const repo = fileURLToPath(new URL("../../../..", import.meta.url));
  const exe = process.platform === "win32" ? "rt-testserver.exe" : "rt-testserver";
  const bin = process.env.RT_TESTSERVER_BIN ?? join(process.env.CARGO_TARGET_DIR ?? join(repo, "target"), "debug", exe);
  if (!existsSync(bin)) throw new Error(`rt-testserver is not built (${bin}); set RT_TESTSERVER_BIN`);
  server2 = spawn(bin, ["--port", String(SERVER2_PORT)], { stdio: "ignore" });
  for (let i = 0; i < 50; i++) {
    const ok = await fetch(`${SERVER2}/status/200`).then((r) => r.ok, () => false);
    if (ok) return;
    await new Promise((r) => setTimeout(r, 100));
  }
  throw new Error("the second rt-testserver did not start");
});

test.afterAll(() => {
  server2?.kill();
});

async function setup(page: Page) {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, files());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Env compare");
  await page.evaluate(
    async ([dev, qa]) => {
      const b = (window as unknown as W).__rt.backend;
      await b.secrets.set("Dev", "token", dev!);
      await b.secrets.set("QA", "token", qa!);
    },
    [CANARY_DEV, CANARY_QA],
  );
}

async function openCompare(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("compare environments");
  await dialog.getByRole("option").filter({ hasText: "Compare environments" }).click();
  await expect(page.getByTestId("envcompare-panel")).toBeVisible();
}

test("two environments answer differently; writes are skipped and no secret shows", async ({ page }) => {
  await setup(page);
  await openCompare(page);
  await page.getByTestId("env-check-Dev").check();
  await page.getByTestId("env-check-QA").check();
  await page.getByTestId("envcompare-start").click();

  const summary = page.getByTestId("envcompare-summary");
  await expect(summary).toBeVisible({ timeout: 30_000 });
  // The echo names its host, so it differs; the status answer is the same.
  await expect(summary).toHaveAttribute("data-total", "2");
  await expect(summary).toHaveAttribute("data-different", "1");
  await page.getByTestId("envcompare-filter-all").click();
  await expect(page.getByTestId("envcompare-row").filter({ hasText: "Status" })).toHaveAttribute("data-same", "true");
  await expect(page.getByTestId("envcompare-row").filter({ hasText: "Echo" })).toHaveAttribute("data-same", "false");
  await expect(page.getByTestId("envcompare-skipped")).toContainText("Create");
  await expect(page.getByTestId("envcompare-skipped")).toContainText("skipped (writes)");

  // Both answers are side by side; the secrets the servers echoed are masked.
  await page.getByTestId("envcompare-row").filter({ hasText: "Echo" }).click();
  await expect(page.getByTestId("cmp-response")).toBeVisible();
  await expect(page.getByTestId("cmp-response")).toContainText("127.0.0.1:" + SERVER.split(":").pop());
  const html = await page.content();
  expect(html).not.toContain(CANARY_DEV);
  expect(html).not.toContain(CANARY_QA);
  // Nothing was written to the collection.
  expect(await readFolderFile(page, "echo.yml")).toContain("{{token}}");
});

test("ignoring the values of the environments' variables makes the hosts the same", async ({ page }) => {
  await setup(page);
  await openCompare(page);
  await page.getByTestId("env-check-Dev").check();
  await page.getByTestId("env-check-QA").check();
  await page.getByTestId("envcompare-ignore").check();
  await page.getByTestId("envcompare-start").click();
  const summary = page.getByTestId("envcompare-summary");
  await expect(summary).toBeVisible({ timeout: 30_000 });
  await expect(summary).toHaveAttribute("data-different", "0");
});
