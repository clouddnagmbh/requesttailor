// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors
import { expect, test } from "@playwright/test";

// The view bar of the sidebar keeps an inset to the edge: in a narrow sidebar it scrolls, and the
// chosen view is never flush with (or behind) the edge.
for (const width of [200, 260, 400]) {
  test(`sidebar views keep their inset at ${width}px`, async ({ page }) => {
    await page.addInitScript((w) => localStorage.setItem("rt.shell.sidebarWidth", JSON.stringify(w)), width);
    await page.setViewportSize({ width: 1000, height: 500 });
    await page.goto("/?backend=mock");
    await page.getByTestId("open-collection").click();
    const side = (await page.locator(".side").boundingBox())!;
    for (const id of ["mcp", "requests", "git"]) {
      const view = page.getByTestId(`view-${id}`);
      await view.click();
      const box = (await view.boundingBox())!;
      expect(side.x + side.width - (box.x + box.width), `${id} right inset`).toBeGreaterThanOrEqual(8);
      expect(box.x - side.x, `${id} left inset`).toBeGreaterThanOrEqual(0);
    }
  });
}
