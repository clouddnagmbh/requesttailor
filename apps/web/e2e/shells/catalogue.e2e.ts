// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 18, strand I in the browser with the real core: the plugin catalogue against the local
// fake index host (testdata/catalogue/fake-index.mjs, ports 48629 and 48630; this file starts
// and stops it). The app asks for nothing at startup, an index without CORS says so and points at
// the companion, a wrong checksum refuses, an update that asks for more rights is confirmed again,
// and a Git source is read through the host's raw address.

import { spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";

import { expect, test, type Page } from "@playwright/test";

import { fakePicker, fillFolder, sampleCollection } from "./env";

const INDEX_PORT = Number(process.env.RT_CATALOGUE_INDEX_PORT ?? 48629);
const GIT_PORT = Number(process.env.RT_CATALOGUE_GIT_PORT ?? 48630);
const HOST = `http://127.0.0.1:${INDEX_PORT}`;
const MOD = process.platform === "darwin" ? "Meta" : "Control";
const script = fileURLToPath(new URL("../../../../testdata/catalogue/fake-index.mjs", import.meta.url));

let host: ChildProcess;

test.beforeAll(async () => {
  host = spawn("node", [script, "--index-port", String(INDEX_PORT), "--git-port", String(GIT_PORT)], { stdio: ["pipe", "pipe", "inherit"] });
  await new Promise<void>((resolve, reject) => {
    host.once("error", reject);
    host.stdout!.on("data", (d: Buffer) => {
      if (d.toString().includes("READY")) resolve();
    });
  });
});

test.afterAll(() => {
  host?.stdin?.end();
  host?.kill();
});

test.beforeEach(async () => {
  await fetch(`${HOST}/__reset`, { method: "POST" });
});

const calls = async (): Promise<string[]> => (await (await fetch(`${HOST}/__calls`)).json()) as string[];
const release = (query: string) => fetch(`${HOST}/__release?${query}`, { method: "POST" });

async function setup(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
}

async function openCatalogue(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Manage plugins");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("plugins-dialog")).toBeVisible();
  await page.getByTestId("plugins-tab-catalogue").click();
}

async function addAndRefresh(page: Page, url: string) {
  await page.getByTestId("catalogue-source-input").fill(url);
  await page.getByTestId("catalogue-add-source").click();
  await expect(page.getByTestId("catalogue-source").last()).toBeVisible();
  await page.getByTestId("catalogue-refresh").click();
}

test("nothing is requested at startup, and an added source costs nothing until Refresh", async ({ page }) => {
  await setup(page);
  await openCatalogue(page);
  await page.getByTestId("catalogue-source-input").fill(`${HOST}/cors/plugins.json`);
  await page.getByTestId("catalogue-add-source").click();
  await expect(page.getByTestId("catalogue-source")).toHaveCount(1);
  expect(await calls(), "the host saw no request").toEqual([]);
  await page.getByTestId("catalogue-refresh").click();
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(1);
  expect(await calls()).toEqual(["GET /cors/plugins.json"]);
  // The source is kept across a reload; the list of plugins is not requested again by itself.
  await page.reload();
  const name = page.getByTestId("collection-name");
  // After a reload the app may already have reopened the folder by itself.
  if (!(await name.isVisible())) await page.getByTestId("open-collection").click();
  await expect(name).toHaveText("Shell test");
  expect(await calls()).toEqual(["GET /cors/plugins.json"]);
  await openCatalogue(page);
  await expect(page.getByTestId("catalogue-source")).toHaveCount(1);
  expect(await calls()).toEqual(["GET /cors/plugins.json"]);
});

test("install checks the checksum, shows the module's rights, and an update with more asks again", async ({ page }) => {
  await setup(page);
  await openCatalogue(page);
  await addAndRefresh(page, `${HOST}/cors/plugins.json`);
  await page.getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-confirm")).toContainText("Install Demo plugin 1.0.0?");
  await expect(page.getByTestId("catalogue-permissions")).toHaveText("Storage: keep its own data on this device");
  await expect(page.getByTestId("catalogue-warning")).toHaveCount(0);
  await page.getByTestId("catalogue-confirm-install").click();
  await expect(page.getByTestId("catalogue-done")).toContainText("Installed demo 1.0.0");

  // The index moves on to 2.0.0, which asks for network access.
  await release("version=2.0.0");
  await page.getByTestId("catalogue-check-updates").click();
  await expect(page.getByTestId("catalogue-update-badge")).toBeVisible();
  await page.getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-confirm")).toContainText("Update Demo plugin 2.0.0?");
  await expect(page.getByTestId("catalogue-more")).toBeVisible();
  await expect(page.getByTestId("catalogue-permissions")).toContainText("Network: the host of the request being sent");
  await page.getByTestId("catalogue-confirm-install").click();
  await expect(page.getByTestId("catalogue-done")).toContainText("Installed demo 2.0.0");
  await page.getByTestId("plugins-tab-installed").click();
  await expect(page.locator('[data-testid=plugin-item][data-plugin="demo"] [data-testid=plugin-version]')).toHaveText("2.0.0");
});

test("a wrong checksum refuses, and a republished release with the same version is refused", async ({ page }) => {
  await setup(page);
  await openCatalogue(page);
  await fetch(`${HOST}/__break?what=checksum`, { method: "POST" });
  await addAndRefresh(page, `${HOST}/cors/plugins.json`);
  await page.getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-error")).toContainText("checksum does not match");
  await expect(page.getByTestId("catalogue-confirm")).toHaveCount(0);

  await fetch(`${HOST}/__break?what=none`, { method: "POST" });
  await page.getByTestId("catalogue-refresh").click();
  await page.getByTestId("catalogue-install").click();
  await page.getByTestId("catalogue-confirm-install").click();
  await expect(page.getByTestId("catalogue-done")).toContainText("Installed demo 1.0.0");

  // The same version, other bytes (with a matching checksum in the index).
  await release("version=1.0.0&tampered=1");
  await page.getByTestId("catalogue-refresh").click();
  await page.getByTestId("catalogue-check-updates").click();
  await page.getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-error")).toContainText("the index changed a published release");
});

test("an index the browser may not read says so and points at the companion", async ({ page }) => {
  await setup(page);
  await openCatalogue(page);
  await addAndRefresh(page, `${HOST}/nocors/plugins.json`);
  await expect(page.getByTestId("catalogue-source-error")).toContainText(/companion/i);
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(0);
});

test("a Git source is read through the host's raw address", async ({ page }) => {
  await setup(page);
  await openCatalogue(page);
  await addAndRefresh(page, `http://127.0.0.1:${GIT_PORT}/org/plugins`);
  await expect(page.getByTestId("catalogue-entry")).toHaveCount(1);
  expect((await calls()).some((c) => c.startsWith("GIT GET /org/plugins/raw/branch/main/plugins.json"))).toBe(true);
  await page.getByTestId("catalogue-install").click();
  await expect(page.getByTestId("catalogue-confirm")).toContainText("Install Demo plugin 1.0.0?");
});
