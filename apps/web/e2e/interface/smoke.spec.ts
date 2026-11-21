// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Interface smoke test: the shell mounts with the mock backend and the command layer works
// end to end (open, select, send). Strands add their own folders next to this one.

import { expect, test } from "@playwright/test";

test("open, select and send with the mock backend", async ({ page }) => {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await page.getByTestId("tree-request").filter({ hasText: "List users" }).click();
  await expect(page.getByTestId("request-editor")).toContainText("{{baseUrl}}/users");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
});
