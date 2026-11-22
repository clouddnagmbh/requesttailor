// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 7, strand D: the collection runner with the real core (rt-wasm) against rt-testserver.
// A folder with three requests, a CSV with two rows: six steps in order, data-row variables
// resolved, the summary in the UI. Declarative assertions are evaluated by strand C's
// `rt_core::assert`; that part is skipped until it is merged.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, sampleCollection } from "./env";

function request(name: string, seq: number, url: string, extra: string[] = []): string {
  return [
    "info:",
    `  name: ${name}`,
    "  type: http",
    `  seq: ${seq}`,
    "",
    "http:",
    "  method: GET",
    `  url: "${url}"`,
    ...extra,
    "",
  ].join("\n");
}

function runCollection(assertions = false): Record<string, string> {
  const status = assertions
    ? [
        "",
        "runtime:",
        "  assertions:",
        "    - expression: res.status",
        "      operator: eq",
        '      value: "200"',
        "    - expression: res.status",
        "      operator: eq",
        '      value: "201"',
      ]
    : [];
  return {
    ...sampleCollection(),
    "api/folder.yml": "info:\n  name: API\n  type: folder\n  seq: 3\n",
    "api/first.yml": request("First", 1, "{{baseUrl}}/echo/user?name={{user}}"),
    "api/second.yml": request("Second", 2, "{{baseUrl}}/status/200", status),
    "api/third.yml": request("Third", 3, "{{baseUrl}}/echo/role/{{role}}"),
  };
}

async function setup(page: Page, assertions = false): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, runCollection(assertions));
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
}

async function runApiWithData(page: Page): Promise<void> {
  const row = page.getByTestId("tree-folder").filter({ hasText: "API" });
  await row.hover();
  await row.getByTestId("run-folder").click();
  await expect(page.getByTestId("runner-title")).toHaveText("Run folder: API");
  await page.getByTestId("runner-data").setInputFiles({
    name: "users.csv",
    mimeType: "text/csv",
    buffer: Buffer.from("user,role\r\nada,admin\r\n\"grace hopper\",dev\r\n"),
  });
  await expect(page.getByTestId("runner-data-name")).toHaveText("users.csv: 2 rows");
  await page.getByTestId("runner-start").click();
  await expect(page.getByTestId("runner-summary")).toHaveAttribute("data-finished", "true", { timeout: 30_000 });
}

test("runs a folder with a CSV through the real core", async ({ page }) => {
  await setup(page);
  await runApiWithData(page);
  const summary = page.getByTestId("runner-summary");
  await expect(summary).toHaveAttribute("data-requests", "6");
  await expect(summary).toHaveAttribute("data-passed", "6");
  await expect(summary).toHaveAttribute("data-failed", "0");
  const steps = page.getByTestId("runner-step");
  await expect(steps).toHaveCount(6);
  await expect(steps.nth(0)).toContainText("First");
  await expect(steps.nth(1)).toContainText("200");
  await expect(steps.nth(5)).toContainText("Third");

  // The data row resolved in the URL of each iteration.
  const details = page.getByTestId("runner-step-details");
  await steps.nth(0).click();
  await expect(details).toContainText(`GET ${SERVER}/echo/user?name=ada`);
  await steps.nth(3).click();
  await expect(details).toContainText(`GET ${SERVER}/echo/user?name=grace%20hopper`);
  await expect(details).toContainText("iteration 2");
  await steps.nth(5).click();
  await expect(details).toContainText(`GET ${SERVER}/echo/role/dev`);
});

test("the command layer runs and plans like the UI", async ({ page }) => {
  await setup(page);
  const result = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const plan = await b.runner.plan("api");
    const data = await b.runner.parseData({ name: "d.json", data: new TextEncoder().encode('[{"user": "x", "role": 1}]') });
    const kinds: string[] = [];
    const summary = await b.runner.run(
      { requests: plan, data, iterations: 1, delay_ms: 0, bail: false },
      { environment: "Local", vars: [] },
      (e) => kinds.push(e.kind),
    );
    let error = "";
    try {
      await b.runner.parseData({ name: "bad.csv", data: new TextEncoder().encode("a,b\n1\n") });
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    return { plan, data, kinds, summary, error };
  });
  expect(result.plan).toEqual(["api/first.yml", "api/second.yml", "api/third.yml"]);
  expect(result.data).toEqual([
    [
      ["user", "x"],
      ["role", 1],
    ],
  ]);
  expect(result.kinds).toEqual(["started", "request", "step", "request", "step", "request", "step", "finished"]);
  expect(result.summary).toMatchObject({ requests: 3, passed: 3, failed: 0, stopped: false });
  expect(result.error).toContain("line 2");
});

test("declarative assertions count in the run", async ({ page }) => {
  await setup(page, true);
  await runApiWithData(page);
  const summary = page.getByTestId("runner-summary");
  await expect(summary).toHaveAttribute("data-failed", "2");
  await expect(summary).toContainText("assertions 2/4");
  await page.locator('[data-testid="runner-step"][data-state="failed"]').first().click();
  await expect(page.getByTestId("runner-assertion")).toHaveCount(2);
});
