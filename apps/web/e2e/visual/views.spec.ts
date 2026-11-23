// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand H: every Git and comparison view in the main area, filled with content, in
// light and dark at 900 and 1440 px. Each must fit: no horizontal scrolling of the page or of
// the view. Screenshots go to `test-results/visual/<view>-<width>-<scheme>.png` (or
// `RT_VISUAL_OUT`) for the review by eye; the test only fails on overflow.

import { mkdirSync } from "node:fs";
import { join } from "node:path";

import { expect, test, type Page } from "@playwright/test";

import type { MockBackend } from "@requesttailor/ui";

type MockGit = import("../../../ui/src/commands/mock-git").MockGit;
type W = Window & { __rt: { backend: MockBackend & { git: MockGit } } };

const MOD = "ControlOrMeta";
const OUT = process.env.RT_VISUAL_OUT ?? join(process.cwd(), "test-results", "visual");

/** The demo collection with history: `main` asserts the health check, `feature/pets` moves it,
 *  edits "List users" and adds "Pets". */
async function setup(page: Page, query = "") {
  await page.goto(`/?backend=mock${query}`);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    await b.git.setIdentity({ name: "Ada Lovelace", email: "ada@example.com" });
    const edit = async (path: string, message: string, change: (s: Awaited<ReturnType<typeof b.getRequest>>) => object) => {
      const spec = await b.getRequest(path);
      await b.updateRequest(path, { ...spec, ...change(spec) });
      await b.save();
      await b.git.commit(message, [path]);
    };
    await edit("health.yml", "Assert 200", () => ({
      assertions: [{ expression: "res.status", operator: "eq", value: "200", description: null, enabled: true }],
    }));
    await b.git.createBranch("feature/pets", true);
    await edit("health.yml", "Health moved", () => ({ url: "{{baseUrl}}/status/503" }));
    await edit("users/list-users.yml", "Second page", (s) => ({
      url: "{{baseUrl}}/users?page=2&include=profile,roles",
      params: s.params.map((p) => ({ ...p, value: "2" })),
    }));
    const pets = await b.createRequest("", "Pets");
    const spec = await b.getRequest(pets);
    await b.updateRequest(pets, { ...spec, url: "{{baseUrl}}/pets" });
    await b.save();
    await b.git.commit("Add pets", [pets]);
  });
}

async function palette(page: Page, query: string, option: string | RegExp) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill(query);
  await dialog.getByRole("option").filter({ hasText: option }).first().click();
}

interface View {
  name: string;
  query?: string;
  open: (page: Page) => Promise<void>;
}

const VIEWS: View[] = [
  {
    name: "compare",
    open: async (page) => {
      await palette(page, "compare rev", "Compare revisions");
      await page.getByTestId("compare-from").fill("main");
      await page.getByTestId("compare-to").fill("feature/pets");
      await page.keyboard.press("Escape");
      await page.getByTestId("compare-start").click();
      await expect(page.getByTestId("cmp-response")).toBeVisible();
      await page.getByTestId("compare-row").filter({ hasText: "Health" }).click();
      await expect(page.getByTestId("cmp-json")).toBeVisible();
    },
  },
  {
    name: "compare-picker",
    open: async (page) => {
      await palette(page, "compare rev", "Compare revisions");
      await page.getByTestId("compare-from").click();
      await expect(page.getByTestId("compare-from-list")).toBeVisible();
    },
  },
  {
    name: "diff",
    open: async (page) => {
      await palette(page, "last commit by field", "Changes: last commit by field");
      await expect(page.getByTestId("semantic-diff-view")).toBeVisible();
    },
  },
  {
    name: "timeline",
    open: async (page) => {
      await page.getByTestId("view-requests").click();
      await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
      await palette(page, "history of this", /History of this request/);
      await expect(page.getByTestId("request-timeline")).toBeVisible();
    },
  },
  {
    name: "bisect",
    open: async (page) => {
      await palette(page, "bisect", "Bisect");
      const first = await page.evaluate(async () => {
        const all = await (window as unknown as W).__rt.backend.git.commits(null, "HEAD", 100);
        return all[all.length - 1]!.oid;
      });
      await page.getByTestId("bisect-good").fill(first);
      await page.getByTestId("bisect-bad").fill("HEAD");
      await page.getByTestId("bisect-start").click();
      await expect(page.getByTestId("bisect-result")).toBeVisible();
    },
  },
  {
    name: "review",
    open: async (page) => {
      await palette(page, "review", "Review: pull and merge requests");
      await page.getByTestId("review-host").fill("git.example.com");
      await page.getByTestId("review-api").fill("https://git.example.com/api/v3");
      await page.getByTestId("review-add-host").click();
      await page.getByTestId("review-token").fill("mock-token");
      await page.getByTestId("review-save-token").click();
      await page.getByTestId("review-row").first().click();
      await expect(page.getByTestId("review-title")).toBeVisible();
    },
  },
  {
    name: "review-list",
    open: async (page) => {
      await palette(page, "review", "Review: pull and merge requests");
      await expect(page.getByTestId("review-no-host")).toBeVisible();
    },
  },
  {
    name: "contract",
    query: "&contract=1",
    open: async (page) => {
      await palette(page, "API contract", "API contract");
      await expect(page.getByTestId("contract-panel")).toBeVisible();
    },
  },
  {
    name: "rescue",
    open: async (page) => {
      await palette(page, "secrets", "Scan history for secrets");
      await page.getByTestId("rescue-scan").click();
      await expect(page.getByTestId("rescue-finding").first()).toBeVisible();
    },
  },
];

const WIDTHS = [900, 1440];
const SCHEMES = ["light", "dark"] as const;

for (const view of VIEWS) {
  test(`${view.name}: light and dark at 900 and 1440 px, no horizontal scrolling`, async ({ page }) => {
    test.setTimeout(90_000);
    mkdirSync(OUT, { recursive: true });
    await page.setViewportSize({ width: 1440, height: 900 });
    await setup(page, view.query ?? "");
    await view.open(page);
    await expect(page.getByTestId("git-view")).toBeVisible();
    for (const width of WIDTHS) {
      await page.setViewportSize({ width, height: 900 });
      for (const scheme of SCHEMES) {
        await page.emulateMedia({ colorScheme: scheme });
        await page.waitForTimeout(100);
        const file = join(OUT, `${view.name}-${width}-${scheme}.png`);
        await page.screenshot({ path: file });
        await test.info().attach(`${view.name}-${width}-${scheme}`, { path: file, contentType: "image/png" });
        const overflow = await page.evaluate(() => {
          const doc = document.documentElement;
          const body = document.querySelector<HTMLElement>('[data-testid="git-view"] .body');
          const view = document.querySelector<HTMLElement>('[data-testid="git-view"]')!.getBoundingClientRect();
          // Visible elements of the view that stick out on the right.
          const out = [...document.querySelectorAll<HTMLElement>('[data-testid="git-view"] *')]
            .filter((el) => {
              const r = el.getBoundingClientRect();
              if (r.width === 0 || r.height === 0) return false;
              const style = getComputedStyle(el);
              if (style.visibility === "hidden" || style.position === "fixed") return false;
              return r.right > view.right + 1 && !el.closest("[data-scrolls-x]");
            })
            .slice(0, 5)
            .map((el) => `${el.tagName.toLowerCase()}.${[...el.classList].join(".")} ${Math.round(el.getBoundingClientRect().right)}`);
          return {
            page: doc.scrollWidth - window.innerWidth,
            body: body ? body.scrollWidth - body.clientWidth : 0,
            out,
          };
        });
        expect(overflow.page, `${view.name} ${width} ${scheme}: page scrolls sideways`).toBeLessThanOrEqual(0);
        expect(overflow.body, `${view.name} ${width} ${scheme}: view scrolls sideways`).toBeLessThanOrEqual(0);
        expect(overflow.out, `${view.name} ${width} ${scheme}: sticks out`).toEqual([]);
      }
    }
  });
}
