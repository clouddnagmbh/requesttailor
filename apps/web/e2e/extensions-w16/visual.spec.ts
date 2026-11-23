// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand C: the suggested-edit view, the editor toolbar, the action rows and the
// assistant panel in light and dark at 900 and 1440 px: nothing overflows the window.
// `RT_SHOTS=<dir>` also writes the screenshots there (for a visual check by a person).

import { expect, test, type Page } from "@playwright/test";

import { expectNoOverflow, openCollection, openRequest } from "./helpers";

const SHOTS = process.env.RT_SHOTS;
const SCRIPT = `// Look up the user, then keep its id for the next request.
const res = await rt.sendRequest({ method: "GET", url: rt.vars.get("baseUrl") + "/users/1" });
rt.vars.set("userId", res.json().id);
rt.vars.set("traceId", crypto.randomUUID());
test("user exists", () => expect(res.status).toBe(200));
`;

const sizes = [
  { width: 1440, height: 900 },
  { width: 900, height: 700 },
] as const;

async function shot(page: Page, name: string, theme: string, width: number) {
  // Let dialogs and toasts finish their fade-in.
  await page.waitForTimeout(400);
  if (SHOTS) await page.screenshot({ path: `${SHOTS}/${name}-${theme}-${width}.png` });
}

for (const scheme of ["light", "dark"] as const) {
  for (const size of sizes) {
    test(`suggested edit, inline and in a dialog, ${scheme} at ${size.width}px`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await page.setViewportSize(size);
      await openCollection(page);
      await openRequest(page);

      // Inline proposal in the script editor.
      await page.getByTestId("tab-scripts").click();
      const editor = page.getByTestId("script-before-request").locator(".cm-content");
      await editor.click();
      await page.keyboard.insertText(SCRIPT);
      await expect(page.getByTestId("editor-shell").first().getByTestId("editor-action")).toBeVisible();
      await shot(page, "editor-toolbar", scheme, size.width);
      await page.getByTestId("editor-shell").first().getByTestId("editor-action").click();
      const view = page.getByTestId("suggested-edit");
      await expect(view).toBeVisible();
      await expect(view.getByTestId("suggest-apply")).toBeVisible();
      await expectNoOverflow(page);
      const box = await view.boundingBox();
      expect(box!.x + box!.width).toBeLessThanOrEqual(size.width);
      await shot(page, "suggest-inline", scheme, size.width);
      await view.getByTestId("suggest-discard").click();

      // Dialog with the field diff of a request.
      await page.locator('[data-action="demo16/header"]').click();
      const dialog = page.getByTestId("extension-dialog");
      await expect(dialog.getByTestId("suggested-edit")).toBeVisible();
      await expectNoOverflow(page);
      const d = await dialog.boundingBox();
      expect(d!.x).toBeGreaterThanOrEqual(0);
      expect(d!.x + d!.width).toBeLessThanOrEqual(size.width);
      await shot(page, "suggest-dialog", scheme, size.width);
      await dialog.getByTestId("suggest-apply").click();
      await expect(page.getByTestId("suggest-toast")).toBeVisible();
      await shot(page, "suggest-applied", scheme, size.width);
      await expectNoOverflow(page);

      // The assistant panel beside (900: over) the editor.
      await page.getByTestId("assistant-toggle").click();
      await expect(page.getByTestId("assistant-panel")).toBeVisible();
      await expectNoOverflow(page);
      await shot(page, "assistant", scheme, size.width);
    });
  }
}
