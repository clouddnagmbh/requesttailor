// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Gate (wave 3): "A blocked request in the web offers the companion with one click."
// Runs against the mock backend: `demo/blocked.yml` fails like a request the browser blocks
// while the transport mode is "browser". The mock's companion is available and paired; the
// other states are stubbed on the page (`__rt.backend.transport.companion`).

import { expect, test, type Page } from "@playwright/test";

const HOW_TO = "Run `rt agent` in a terminal, then try again.";

async function sendBlocked(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Blocked by the browser" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("companion-offer")).toBeVisible();
}

const mode = (page: Page) =>
  page.evaluate(() => (window as unknown as RtWindow).__rt.backend.transport.mode());

interface RtWindow {
  __rt: { backend: import("@requesttailor/ui").Backend };
  __companion: { available: boolean; paired: boolean };
}

/** Replaces the mock's companion status with `window.__companion`. A typed pairing code
 *  `ABCD-EFGH` (the `#rt-pair=` fragment, like the real client reads it) pairs. */
async function stubCompanion(page: Page, state: { available: boolean; paired: boolean }) {
  await page.evaluate(
    ({ state, howTo }) => {
      const w = window as unknown as RtWindow;
      w.__companion = state;
      w.__rt.backend.transport.companion = async () => ({
        ...w.__companion,
        url: w.__companion.available ? "http://127.0.0.1:47812" : null,
        howTo,
      });
      window.addEventListener("hashchange", () => {
        if (location.hash === "#rt-pair=ABCD-EFGH") w.__companion.paired = true;
      });
    },
    { state, howTo: HOW_TO },
  );
}

const setCompanion = (page: Page, state: { available: boolean; paired: boolean }) =>
  page.evaluate((state) => void ((window as unknown as RtWindow).__companion = state), state);

test("a blocked request offers the companion and one click resends through it", async ({ page }) => {
  await sendBlocked(page);
  const offer = page.getByTestId("companion-offer");
  await expect(offer).toContainText(
    "The browser blocked this request. Send it through the RequestTailor companion?",
  );
  await expect(page.getByTestId("response-error")).toContainText("blocked");
  // Never switched without the click.
  expect(await mode(page)).toBe("browser");

  await page.getByTestId("companion-use").click();

  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(offer).toBeHidden();
  expect(await mode(page)).toBe("companion");
});

test("not running: shows how to start rt agent, copies the command, retries", async ({ page, context }) => {
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await sendBlocked(page);
  await stubCompanion(page, { available: false, paired: false });

  await page.getByTestId("companion-use").click();
  const howTo = page.getByTestId("companion-howto");
  await expect(howTo).toContainText("The companion is not running.");
  await expect(howTo.locator("code")).toHaveText("rt agent");
  expect(await mode(page)).toBe("browser");

  await page.getByTestId("companion-copy").click();
  await expect(page.getByTestId("companion-copy")).toHaveText("Copied");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("rt agent");

  // Still not running: stays on the how-to.
  await page.getByTestId("companion-retry").click();
  await expect(howTo).toBeVisible();
  expect(await mode(page)).toBe("browser");

  // The user started `rt agent` (and is paired): retry resends.
  await setCompanion(page, { available: true, paired: true });
  await page.getByTestId("companion-retry").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("companion-offer")).toBeHidden();
  expect(await mode(page)).toBe("companion");
});

test("running but not paired: pairs with the typed code, then resends", async ({ page }) => {
  await sendBlocked(page);
  await stubCompanion(page, { available: true, paired: false });

  await page.getByTestId("companion-use").click();
  await expect(page.getByTestId("companion-offer")).toContainText("open the pairing link");
  const input = page.getByTestId("companion-code");
  await expect(page.getByTestId("companion-pair")).toBeDisabled();

  await input.fill("0000-0000");
  await page.getByTestId("companion-pair").click();
  await expect(page.getByTestId("companion-problem")).toContainText("did not accept");
  expect(await mode(page)).toBe("browser");

  await input.fill("ABCD-EFGH");
  await page.getByTestId("companion-pair").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("companion-offer")).toBeHidden();
  expect(await mode(page)).toBe("companion");
});

test("running but not paired: retry after opening the pairing link", async ({ page }) => {
  await sendBlocked(page);
  await stubCompanion(page, { available: true, paired: false });
  await page.getByTestId("companion-use").click();
  await expect(page.getByTestId("companion-code")).toBeVisible();

  // Paired in another tab through the link.
  await setCompanion(page, { available: true, paired: true });
  await page.getByTestId("companion-retry").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  expect(await mode(page)).toBe("companion");
});

test("requests the browser can send show no offer", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await expect(page.getByTestId("companion-offer")).toHaveCount(0);
});
