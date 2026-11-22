// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 12, strand C: branch compare and bisect in the real web backend: rt-wasm runs the
// requests of a commit (read by isomorphic-git from an OPFS folder, no checkout) against
// rt-testserver, with today's environment. Four commits: the check goes to /status/500 in the
// third. Run: see playwright.real.config.ts.

import { expect, test, type Page } from "@playwright/test";

import type { Backend } from "@requesttailor/ui";

import { fakePicker, fillFolder, readFolderFile } from "../shells/env";
import { SERVER_PORT } from "./playwright.real.config";

const SERVER = `http://127.0.0.1:${SERVER_PORT}`;
const MOD = "ControlOrMeta";

type W = Window & { __rt: { backend: Backend } };

const request = (name: string, seq: number, url: string, assertion: boolean) =>
  [
    "info:",
    `  name: ${name}`,
    "  type: http",
    `  seq: ${seq}`,
    "",
    "http:",
    "  method: GET",
    `  url: "${url}"`,
    ...(assertion
      ? ["", "runtime:", "  assertions:", "    - expression: res.status", "      operator: eq", '      value: "200"']
      : []),
    "",
  ].join("\n");

const check = (status: number) => request("Check", 1, `{{baseUrl}}/status/${status}`, true);
const echo = (x: number) => request("Echo", 2, `{{baseUrl}}/echo/get?x=${x}`, false);

/** Opens an OPFS collection, makes four commits and returns their ids, oldest first. */
async function setup(page: Page): Promise<string[]> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Compare test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n`,
    "check.yml": check(200),
    "echo.yml": echo(1),
  });
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Compare test");
  return page.evaluate(
    async ({ files }) => {
      const b = (window as unknown as W).__rt.backend;
      await b.git.init();
      await b.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
      const ids: string[] = [];
      ids.push((await b.git.commit("Initial collection", files)).oid);
      const edit = async (path: string, url: string, message: string) => {
        const spec = await b.getRequest(path);
        await b.updateRequest(path, { ...spec, url });
        await b.save();
        ids.push((await b.git.commit(message, [path])).oid);
      };
      await edit("echo.yml", "{{baseUrl}}/echo/get?x=2", "Echo asks for 2");
      await edit("check.yml", "{{baseUrl}}/status/500", "Check moved");
      await edit("echo.yml", "{{baseUrl}}/echo/get?x=4", "Echo asks for 4");
      return ids;
    },
    { files: ["opencollection.yml", "environments/Local.yml", "check.yml", "echo.yml"] },
  );
}

const input = { environment: "Local", vars: [] as [string, string][] };

test("runs a commit without a checkout, with today's environment", async ({ page }) => {
  const ids = await setup(page);
  const run = await page.evaluate(
    ({ rev, input }) => (window as unknown as W).__rt.backend.compare.run({ rev }, input),
    { rev: ids[0]!, input },
  );
  expect(run.commit.oid).toBe(ids[0]);
  expect(run.commit.summary).toBe("Initial collection");
  expect(run.summary).toMatchObject({ requests: 2, passed: 2, failed: 0, assertions_passed: 1 });
  expect(run.steps[0]!.report.request!.url).toBe(`${SERVER}/status/200`);

  const head = await page.evaluate(
    ({ input }) => (window as unknown as W).__rt.backend.compare.run({ rev: "HEAD", folder: null }, input),
    { input },
  );
  expect(head.summary).toMatchObject({ failed: 1, assertions_failed: 1 });
  // Nothing was checked out: the working folder still has the newest files.
  expect(await readFolderFile(page, "check.yml")).toContain("/status/500");
  await expect(page.getByTestId("collection-name")).toHaveText("Compare test");
});

test("compares two commits in the view, per request", async ({ page }) => {
  const ids = await setup(page);
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("compare rev");
  await dialog.getByRole("option").filter({ hasText: "Compare revisions" }).click();
  await page.getByTestId("compare-from").fill(ids[1]!);
  await page.getByTestId("compare-to").fill("HEAD");
  await page.getByTestId("compare-start").click();
  await expect(page.getByTestId("compare-changed")).toHaveText("2 changed");
  const rows = page.getByTestId("compare-row");
  await expect(rows.filter({ hasText: "Check" })).toContainText("status 200 → 500");
  await rows.filter({ hasText: "Check" }).getByRole("button").click();
  await expect(page.getByTestId("compare-tests")).toContainText("res.status eq 200");
  await expect(page.getByTestId("compare-tests")).toContainText("failed");
  // The echo server repeats the query: its body differs at one value, status and tests do not.
  await rows.filter({ hasText: "Echo" }).getByRole("button").click();
  const echoBody = page.getByTestId("compare-body").filter({ hasText: "/query" });
  await expect(echoBody).toContainText('"x=2"');
  await expect(echoBody).toContainText('"x=4"');
});

test("bisect finds the commit that broke the check, in few runs", async ({ page }) => {
  const ids = await setup(page);
  const outcome = await page.evaluate(
    async ({ good, input }) => {
      const steps: string[] = [];
      const outcome = await (window as unknown as W).__rt.backend.compare.bisect(
        { good, bad: "HEAD" },
        input,
        (s) => steps.push(s.verdict),
      );
      return { outcome, steps };
    },
    { good: ids[0]!, input },
  );
  expect(outcome.outcome.first_bad?.oid).toBe(ids[2]);
  expect(outcome.outcome.candidates).toBe(3);
  expect(outcome.outcome.steps.length).toBeLessThanOrEqual(3);
  expect(outcome.steps).toEqual(outcome.outcome.steps.map((s) => s.verdict));
  expect(outcome.outcome.steps[0]!.failed[0]).toMatch(/^Check: res\.status eq 200/);

  // Each tested commit's run went to the time travel under that commit (IndexedDB).
  const recorded = await page.evaluate(async (oids) => {
    const b = (window as unknown as W).__rt.backend;
    for (let i = 0; i < 50; i++) {
      const all = await Promise.all(oids.map((oid) => b.timeline.results(oid)));
      if (all.every((r) => r.length > 0)) return all;
      await new Promise((r) => setTimeout(r, 50));
    }
    return Promise.all(oids.map((oid) => b.timeline.results(oid)));
  }, outcome.outcome.steps.map((s) => s.commit.oid));
  for (const records of recorded) {
    expect(records.some((r) => r.source === "bisect")).toBe(true);
  }
  const atHead = recorded[0]!.find((r) => r.request === "check.yml");
  expect(atHead).toMatchObject({ passed: false, status: 500, source: "bisect" });

  // The same in the view.
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("bisect");
  await dialog.getByRole("option").filter({ hasText: "Bisect" }).click();
  await page.getByTestId("bisect-good").fill(ids[0]!);
  await page.getByTestId("bisect-start").click();
  await expect(page.getByTestId("bisect-result")).toContainText(`First bad commit: ${ids[2]!.slice(0, 7)} Check moved`);
});

test("an unknown revision is a clear error", async ({ page }) => {
  await setup(page);
  const message = await page.evaluate(({ input }) =>
    (window as unknown as W).__rt.backend.compare
      .run({ rev: "no-such-branch" }, input)
      .then(() => "no error", (e: Error) => e.message),
    { input },
  );
  expect(message).toMatch(/no-such-branch/i);
});
