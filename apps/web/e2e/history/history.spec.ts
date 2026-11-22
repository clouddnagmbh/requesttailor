// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 4, strand E: the History view (mock backend). Every send shows up, newest first, with
// method, name, status coloured by class, time and duration; the filter narrows it; click and
// keyboard open the request; "Clear history" asks inside the panel.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const treeRequest = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name });
const tab = (page: Page, name: string) => page.getByTestId("tab").filter({ hasText: name });
const entries = (page: Page) => page.getByTestId("history-entry");

async function sendFromTree(page: Page, name: string) {
  await page.getByTestId("view-requests").click();
  await treeRequest(page, name).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status").or(page.getByTestId("response-error"))).toBeVisible();
}

/** Adds entries with the given statuses (`null`: no response) straight into the mock. */
async function seed(page: Page, rows: { path: string; status: number | null; minutesAgo?: number }[]) {
  await page.evaluate(async (rows) => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").MockBackend } }).__rt.backend;
    const history = b.history as unknown as {
      clock: () => number;
      record(c: string, s: unknown, r: unknown): void;
    };
    for (const row of rows) {
      const spec = await b.getRequest(row.path);
      history.clock = () => Date.now() - (row.minutesAgo ?? 0) * 60_000;
      history.record(
        "mock-collection",
        spec,
        {
          report: {
            version: 1,
            request_path: row.path,
            environment: "Local",
            request: { method: spec.method, url: `https://api.example.com/${row.path}`, headers: [], body_size: 0 },
            response:
              row.status == null
                ? null
                : {
                    status: row.status,
                    version: "HTTP/1.1",
                    headers: [],
                    url: "",
                    redirects: [],
                    body: { encoding: "utf8", data: "" },
                    size: 2,
                    truncated: false,
                    timing: { dns: null, connect: null, tls: null, ttfb: 1 },
                    total: 250,
                  },
            variables: [],
            unresolved: [],
            error: row.status == null ? { kind: "transport", message: { kind: "timeout" } } : null,
          },
          response_id: null,
          size: 0,
          line_count: 0,
          is_text: false,
        },
      );
    }
    history.clock = () => Date.now();
  }, rows);
}

test("every send appears newest first with method, name, status, time and duration", async ({ page }) => {
  await openCollection(page);
  await page.getByTestId("view-history").click();
  await expect(page.getByTestId("history-empty")).toHaveText("Nothing sent from this collection yet.");

  await sendFromTree(page, "List users");
  await sendFromTree(page, "Create user");
  await sendFromTree(page, "Blocked by the browser");

  await page.getByTestId("view-history").click();
  await expect(entries(page)).toHaveCount(3);
  await expect(entries(page).nth(0)).toContainText("Blocked by the browser");
  await expect(entries(page).nth(1)).toContainText("POST");
  await expect(entries(page).nth(1)).toContainText("Create user");
  await expect(entries(page).nth(2)).toContainText("List users");
  const first = entries(page).nth(2);
  await expect(first.getByTestId("history-status")).toHaveText("200");
  await expect(first.getByTestId("history-status")).toHaveAttribute("data-class", "ok");
  await expect(first.getByTestId("history-time")).toHaveText("just now");
  await expect(first.getByTestId("history-duration")).toHaveText("14 ms");
  // The failed send is there too, with its error instead of a status.
  await expect(entries(page).nth(0).getByTestId("history-status")).toHaveText("Error");
  await expect(entries(page).nth(0)).toContainText("The browser blocked the request");

  // While the view is open, a new send shows up at once.
  await page.getByTestId("tab").filter({ hasText: "List users" }).click();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(entries(page)).toHaveCount(4);
  await expect(entries(page).nth(0)).toContainText("List users");
});

test("status is coloured by class and time is relative", async ({ page }) => {
  await openCollection(page);
  await seed(page, [
    { path: "health.yml", status: 200, minutesAgo: 120 },
    { path: "users/list-users.yml", status: 302, minutesAgo: 5 },
    { path: "users/create-user.yml", status: 404, minutesAgo: 2 },
    { path: "demo/large.yml", status: 503 },
    { path: "demo/blocked.yml", status: null },
  ]);
  await page.getByTestId("view-history").click();
  await expect(entries(page)).toHaveCount(5);
  const classes = await page.getByTestId("history-status").evaluateAll((els) => els.map((e) => e.getAttribute("data-class")));
  expect(classes).toEqual(["error", "server-error", "client-error", "redirect", "ok"]);
  await expect(page.getByTestId("history-time")).toHaveText(["just now", "just now", "2 min ago", "5 min ago", "2 h ago"]);
  await expect(entries(page).nth(1).getByTestId("history-duration")).toHaveText("250 ms");
  // The colours differ (tokens, not one colour for all).
  const colors = await page
    .getByTestId("history-status")
    .evaluateAll((els) => els.map((e) => getComputedStyle(e).color));
  expect(new Set(colors).size).toBeGreaterThanOrEqual(4);
});

test("the filter matches name, method, URL, status and class", async ({ page }) => {
  await openCollection(page);
  await seed(page, [
    { path: "health.yml", status: 200 },
    { path: "users/list-users.yml", status: 404 },
    { path: "users/create-user.yml", status: 500 },
  ]);
  await page.getByTestId("view-history").click();
  const filter = page.getByTestId("history-filter");
  const names = () => entries(page).locator(".name");

  await filter.fill("user");
  await expect(names()).toHaveText(["Create user", "List users"]);
  await filter.fill("post");
  await expect(names()).toHaveText(["Create user"]);
  await filter.fill("4xx");
  await expect(names()).toHaveText(["List users"]);
  await filter.fill("500");
  await expect(names()).toHaveText(["Create user"]);
  await filter.fill("health.yml");
  await expect(names()).toHaveText(["Health"]);
  await filter.fill("nothing-like-this");
  await expect(entries(page)).toHaveCount(0);
  await expect(page.getByTestId("history-empty")).toHaveText("No sends match the filter.");
  // Escape clears the filter.
  await filter.press("Escape");
  await expect(filter).toHaveValue("");
  await expect(entries(page)).toHaveCount(3);
});

test("click and keyboard open the request in a tab", async ({ page }) => {
  await openCollection(page);
  await seed(page, [
    { path: "health.yml", status: 200 },
    { path: "users/list-users.yml", status: 200 },
    { path: "users/create-user.yml", status: 200 },
  ]);
  await page.getByTestId("view-history").click();
  await entries(page).filter({ hasText: "Health" }).click();
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");

  // From the filter field: ArrowDown into the list, arrows, Enter.
  await page.getByTestId("history-filter").focus();
  await page.keyboard.press("ArrowDown");
  await expect(entries(page).nth(0)).toBeFocused();
  await page.keyboard.press("ArrowDown");
  await expect(entries(page).nth(1)).toBeFocused();
  await expect(entries(page).nth(1)).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("Enter");
  await expect(tab(page, "List users")).toHaveAttribute("aria-selected", "true");
  await page.keyboard.press("End");
  await expect(entries(page).nth(2)).toBeFocused();
  await page.keyboard.press("Home");
  await expect(entries(page).nth(0)).toBeFocused();
  await page.keyboard.press(" ");
  await expect(tab(page, "Create user")).toHaveAttribute("aria-selected", "true");
  // One tab stop for the whole list.
  await expect(page.locator('[data-testid="history-entry"][tabindex="0"]')).toHaveCount(1);
});

test("clear history asks inside the panel, Cancel keeps it", async ({ page }) => {
  await openCollection(page);
  await seed(page, [
    { path: "health.yml", status: 200 },
    { path: "users/list-users.yml", status: 200 },
  ]);
  let dialogs = 0;
  page.on("dialog", (d) => {
    dialogs++;
    void d.dismiss();
  });
  await page.getByTestId("view-history").click();
  await expect(entries(page)).toHaveCount(2);

  await page.getByTestId("history-clear").click();
  const confirm = page.getByTestId("history-confirm");
  await expect(confirm).toBeVisible();
  await expect(page.getByTestId("history-clear-confirm")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(confirm).toBeHidden();
  await expect(page.getByTestId("history-clear")).toBeFocused();
  await page.getByTestId("history-clear").click();
  await page.getByTestId("history-clear-cancel").click();
  await expect(entries(page)).toHaveCount(2);

  await page.getByTestId("history-clear").click();
  await page.getByTestId("history-clear-confirm").click();
  await expect(entries(page)).toHaveCount(0);
  await expect(page.getByTestId("history-empty")).toBeVisible();
  expect(dialogs).toBe(0);
  // Cleared in the backend too.
  await page.getByTestId("view-requests").click();
  await page.getByTestId("view-history").click();
  await expect(page.getByTestId("history-empty")).toBeVisible();
});

test("the palette command Show history opens the view", async ({ page }) => {
  await openCollection(page);
  await page.keyboard.press(`${MOD}+k`);
  await page.getByTestId("palette-input").fill("Show history");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("history-panel")).toBeVisible();
  await expect(page.getByTestId("view-history")).toHaveAttribute("aria-selected", "true");
});
