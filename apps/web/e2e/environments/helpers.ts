// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shared steps for strand D's tests (environments and secrets, `?backend=mock`).

import { expect, type Page } from "@playwright/test";

import type { Environment, MockBackend, SendReport } from "@requesttailor/ui";

export type TestWindow = Window & {
  __rt: { backend: MockBackend };
  /** Reports of sends started by the app (not by previews), oldest first. */
  __sends: SendReport[];
};

/** Opens the demo collection and records every report of `backend.send` the app makes. */
export async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.evaluate(() => {
    const w = window as unknown as TestWindow;
    const b = w.__rt.backend;
    const send = b.send.bind(b);
    w.__sends = [];
    // The mock's `preview` calls `this.send`; give it the unrecorded original.
    const previewThis = Object.assign(Object.create(b) as MockBackend, { send });
    b.preview = (path, input) => Object.getPrototypeOf(b).preview.call(previewThis, path, input);
    b.send = async (path, input) => {
      const result = await send(path, input);
      w.__sends.push(result.report);
      return result;
    };
  });
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

/** Clicks Send and returns the report of that send. */
export async function sendAndReport(page: Page): Promise<SendReport> {
  const before = await page.evaluate(() => (window as unknown as TestWindow).__sends.length);
  await page.getByTestId("send").click();
  await expect
    .poll(() => page.evaluate(() => (window as unknown as TestWindow).__sends.length))
    .toBeGreaterThan(before);
  return page.evaluate(() => {
    const s = (window as unknown as TestWindow).__sends;
    return s[s.length - 1];
  });
}

export async function openManager(page: Page) {
  await page.getByRole("button", { name: "Environments" }).click();
  const manager = page.getByTestId("env-manager");
  await expect(manager).toBeVisible();
  return manager;
}

export const environments = (page: Page): Promise<Environment[]> =>
  page.evaluate(() => (window as unknown as TestWindow).__rt.backend.environments());

export const secret = (page: Page, env: string, name: string): Promise<string | null> =>
  page.evaluate(([e, n]) => (window as unknown as TestWindow).__rt.backend.secrets.get(e, n), [env, name]);

/** All files the mock wrote on save, as `[path, text]`. */
export const files = (page: Page): Promise<[string, string][]> =>
  page.evaluate(() => [...(window as unknown as TestWindow).__rt.backend.files.entries()]);
