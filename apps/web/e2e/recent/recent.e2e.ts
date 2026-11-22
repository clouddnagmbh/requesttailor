// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Reopen at start with the real web backend: rt-wasm, a folder in OPFS behind a fake picker,
// directory handles and the recent list in IndexedDB, requests to rt-testserver.

import { expect, test, type Page } from "@playwright/test";

import { fakePicker, fillFolder, sampleCollection } from "../shells/env";
import { writePerfResult } from "../perf/frames";

const MOD = "ControlOrMeta";
const tab = (page: Page, name: string) => page.getByTestId("tab").filter({ hasText: name });

/** From the next load on, the folder picker must not be needed. */
async function noPicker(page: Page) {
  await page.addInitScript(() => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => {
        throw new Error("picker should not open");
      },
    });
  });
}

async function firstSession(page: Page) {
  await fakePicker(page);
  await page.goto("/");
  await fillFolder(page, sampleCollection());
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
}

test("keystroke budget: after a restart one keystroke sends the last request", async ({ page }) => {
  await firstSession(page);
  await noPicker(page);
  await page.reload();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
  let keystrokes = 0;
  keystrokes++;
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByRole("alert")).toHaveCount(0);
  expect(keystrokes).toBe(1);
  writePerfResult("keystrokes-web", { send_keystrokes: keystrokes });
});

test("the recent list remembers names; a folder that needs consent waits for a click", async ({ page }) => {
  await firstSession(page);
  const listed = await page.evaluate(async () => {
    const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
    return b.recentCollections();
  });
  expect(listed).toHaveLength(1);
  expect(listed[0]).toMatchObject({ name: "Shell test", location: "collection" });

  // Next visit: the browser wants to ask again (no "allow on every visit"). Nothing opens on
  // its own; the start screen offers the folder and one click (with the prompt) opens it.
  await noPicker(page);
  await page.addInitScript(() => {
    const proto = FileSystemHandle.prototype as unknown as Record<string, unknown>;
    let granted = false;
    proto.queryPermission = async () => (granted ? "granted" : "prompt");
    proto.requestPermission = async () => {
      granted = true;
      return "granted";
    };
  });
  await page.reload();
  const item = page.getByTestId("recent-collection").filter({ hasText: "Shell test" });
  await expect(item).toBeVisible();
  await expect(page.getByTestId("collection-name")).toHaveText("");
  await item.click();
  await expect(page.getByTestId("collection-name")).toHaveText("Shell test");
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
});

test("a folder that is gone is dropped from the list", async ({ page }) => {
  await firstSession(page);
  await page.evaluate(async () => (await navigator.storage.getDirectory()).removeEntry("collection", { recursive: true }));
  await noPicker(page);
  await page.reload();
  await expect(page.getByTestId("start")).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(async () => {
        const b = (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend;
        return (await b.recentCollections()).length;
      }),
    )
    .toBe(0);
  await expect(page.getByTestId("collection-name")).toHaveText("");
  await expect(page.getByRole("alert")).toHaveCount(0);
});
