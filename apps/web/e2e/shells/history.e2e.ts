// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand E in the browser with the real core: every send is recorded in IndexedDB
// (rt-wasm's HistoryLog), survives a reload, is cleared per collection, works in memory when
// IndexedDB is unavailable, and never holds a secret value: not in IndexedDB, not in the
// list output. Search runs in the core too.

import { expect, test, type Page } from "@playwright/test";

import { SERVER, fakePicker, fillFolder, sampleCollection } from "./env";

const SECRET = "s3cr3tWebHist42"; // gitleaks:allow (dummy value for the secret test)

async function setup(page: Page): Promise<void> {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
}

/** Everything in the history database, as JSON text. */
async function rawHistory(page: Page): Promise<string> {
  return page.evaluate(async () => {
    const db = await new Promise<IDBDatabase>((resolve, reject) => {
      const open = indexedDB.open("requesttailor-history");
      open.onsuccess = () => resolve(open.result);
      open.onerror = () => reject(open.error);
    });
    try {
      if (!db.objectStoreNames.contains("entries")) return "[]";
      const all = await new Promise<unknown[]>((resolve, reject) => {
        const r = db.transaction("entries", "readonly").objectStore("entries").getAll();
        r.onsuccess = () => resolve(r.result);
        r.onerror = () => reject(r.error);
      });
      return JSON.stringify(all);
    } finally {
      db.close();
    }
  });
}

test("sends with a secret in query, path, header and auth: nothing secret in IndexedDB or the list", async ({
  page,
}) => {
  await setup(page);
  const result = await page.evaluate(
    async ({ secret, url }) => {
      const { createWebBackend } = await import(url);
      // In-memory stand-in for the browser secret store.
      const values = new Map<string, string>();
      const k = (e: string, v: string) => `${e}\u0000${v}`;
      const store = {
        has: async (e: string, v: string) => values.has(k(e, v)),
        get: async (e: string, v: string) => values.get(k(e, v)) ?? null,
        set: async (e: string, v: string, value: string) => void values.set(k(e, v), value),
        delete: async (e: string, v: string) => void values.delete(k(e, v)),
      };
      const b = (await createWebBackend({ openSecrets: async () => store })) as import("@requesttailor/ui").Backend;
      const info = await b.openCollection();
      await b.secrets.set("Local", "token", secret);
      const add = async (name: string, patch: Partial<import("@requesttailor/ui").RequestSpec>) => {
        const path = await b.createRequest("", name);
        await b.updateRequest(path, { ...(await b.getRequest(path)), name, ...patch });
        return path;
      };
      const cases: [string, Partial<import("@requesttailor/ui").RequestSpec>][] = [
        ["Query", { url: "{{baseUrl}}/echo/q?key={{token}}" }],
        ["Path and bearer", { url: "{{baseUrl}}/bearer/{{token}}", auth: { type: "bearer", token: "{{token}}" } }],
        [
          "Header",
          { url: "{{baseUrl}}/echo/h", headers: [{ name: "X-Token", value: "pre-{{token}}", enabled: true }] },
        ],
        [
          "Basic",
          { url: "{{baseUrl}}/basic-auth/ada/{{token}}", auth: { type: "basic", username: "ada", password: "{{token}}" } },
        ],
        [
          "API key",
          {
            url: "{{baseUrl}}/status/500",
            auth: { type: "api-key", key: "api_key", value: "{{token}}", placement: "query" },
          },
        ],
        ["Bad scheme", { url: "ftp://example.test/{{token}}" }],
      ];
      const statuses: (number | null)[] = [];
      for (const [name, patch] of cases) {
        const path = await add(name, patch);
        const sent = await b.send(path, { environment: "Local", vars: [] });
        statuses.push(sent.report.response?.status ?? null);
      }
      const listed = await b.history.list("", 100);
      const filtered = await b.history.list("5xx", 100);
      const bySecret = await b.history.list(secret, 100);
      return { id: info!.id, statuses, listed, filtered, bySecret };
    },
    { secret: SECRET, url: "/src/web-backend.ts" },
  );

  // The secret really went over the wire: the server checked it (bearer, basic).
  expect(result.statuses).toEqual([200, 200, 200, 200, 500, null]);
  expect(result.listed.map((e) => e.name)).toEqual([
    "Bad scheme",
    "API key",
    "Basic",
    "Header",
    "Path and bearer",
    "Query",
  ]);
  expect(result.listed.every((e) => e.collection === result.id && e.environment === "Local")).toBe(true);
  expect(result.listed[5].url).toBe(`${SERVER}/echo/q?key=***`);
  expect(result.listed[5].status).toBe(200);
  expect(result.listed[0].error).not.toBeNull();
  expect(result.filtered.map((e) => e.name)).toEqual(["API key"]);
  expect(result.bySecret).toEqual([]);

  const listedText = JSON.stringify(result.listed);
  const raw = await rawHistory(page);
  expect(JSON.parse(raw)).toHaveLength(6);
  for (const text of [listedText, raw]) {
    expect(text).not.toContain(SECRET);
    expect(text).not.toContain("s3cr3t");
    expect(text).not.toContain(Buffer.from(`ada:${SECRET}`).toString("base64"));
    // No bodies, no header values.
    expect(text).not.toContain("authenticated");
    expect(text).not.toContain("pre-");
  }
});

test("in the UI: the history shows sends, survives a reload, and clears", async ({ page }) => {
  await setup(page);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.getByTestId("tree-request").filter({ hasText: "Refused" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-error")).toBeVisible();

  await page.getByTestId("view-history").click();
  const entries = page.getByTestId("history-entry");
  await expect(entries).toHaveCount(2);
  await expect(entries.nth(0)).toContainText("Refused");
  await expect(entries.nth(0).getByTestId("history-status")).toHaveText("Error");
  await expect(entries.nth(1)).toContainText("Health");
  await expect(entries.nth(1).getByTestId("history-status")).toHaveText("200");
  await expect(entries.nth(1).getByTestId("history-status")).toHaveAttribute("data-class", "ok");

  // After a reload (same folder, same collection id) the history is still there.
  await page.reload();
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("view-history").click();
  await expect(entries).toHaveCount(2);

  // Opening from the history opens the request.
  await entries.nth(1).click();
  await expect(page.getByTestId("tab").filter({ hasText: "Health" })).toHaveAttribute("aria-selected", "true");

  await page.getByTestId("history-clear").click();
  await page.getByTestId("history-clear-confirm").click();
  await expect(entries).toHaveCount(0);
  expect(JSON.parse(await rawHistory(page))).toEqual([]);
});

test("without IndexedDB for the history it still works in memory", async ({ page }) => {
  await page.addInitScript(() => {
    const open = IDBFactory.prototype.open;
    IDBFactory.prototype.open = function (this: IDBFactory, name: string, version?: number) {
      if (name === "requesttailor-history") throw new DOMException("blocked for the test", "SecurityError");
      return version === undefined ? open.call(this, name) : open.call(this, name, version);
    };
  });
  await setup(page);
  await page.getByTestId("open-collection").click();
  await page.getByTestId("env-switcher").selectOption("Local");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.getByTestId("view-history").click();
  await expect(page.getByTestId("history-entry")).toHaveCount(1);
  await expect(page.getByRole("alert")).toHaveCount(0);
});

test("search runs in the core and finds unsaved edits", async ({ page }) => {
  await setup(page);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  const hits = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, name: "Gesundheit 🚀 Ärzte" });
    return b.search("ärzte", 50);
  });
  expect(hits).toEqual([
    {
      path: "health.yml",
      name: "Gesundheit 🚀 Ärzte",
      method: "GET",
      field: "name",
      snippet: "Gesundheit 🚀 Ärzte",
      ranges: [[14, 19]],
    },
  ]);

  await page.getByTestId("view-search").click();
  await page.getByTestId("search-input").fill("echo/health");
  await expect(page.getByTestId("search-hit")).toHaveCount(1);
  await expect(page.getByTestId("search-hit").locator("mark")).toHaveText("echo/health");
  await page.getByTestId("search-input").press("Enter");
  await expect(page.getByTestId("tab").filter({ hasText: "Gesundheit" })).toHaveAttribute("aria-selected", "true");
});
