// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand C: the test-data journal against the mock backend (`?backend=mock`). It is
// off until "Track test data" is switched on; then every successful writing send is listed;
// "Revert…" fills {{journal.id}} into the request the user picks, shows the plan (masked URL,
// effect, environment) and sends it on Enter; production asks first and Esc sends nothing.

import { expect, test, type Page } from "@playwright/test";

import type { Backend } from "@requesttailor/ui";

type W = Window & { __rt: { backend: Backend } };

const MOD = "ControlOrMeta";

async function openDemo(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

async function openJournal(page: Page) {
  await page.keyboard.press(`${MOD}+k`);
  const dialog = page.getByRole("dialog", { name: "Command palette" });
  await dialog.getByRole("combobox").fill("test-data journal");
  await dialog.getByRole("option").filter({ hasText: "Test-data journal" }).click();
  await expect(page.getByTestId("journal-panel")).toBeVisible();
}

const request = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name }).click();

/** Turns the DELETE-with-the-journal-id request on: "Health" becomes `DELETE /users/{{journal.id}}` (the tree still shows its saved name). */
async function makeRemoveRequest(page: Page) {
  await page.evaluate(async () => {
    const b = (window as unknown as W).__rt.backend;
    const spec = await b.getRequest("health.yml");
    await b.updateRequest("health.yml", { ...spec, name: "Remove user", method: "DELETE", url: "{{baseUrl}}/users/{{journal.id}}" });
  });
}

async function trackOn(page: Page) {
  await openJournal(page);
  await page.getByTestId("journal-track").check();
  await page.getByTestId("journal-id-path").fill("$.id");
  await page.getByTestId("journal-id-path").blur();
}

test("the journal is off until switched on and records nothing before", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await openJournal(page);
  await expect(page.getByTestId("journal-empty")).toContainText("The journal is off");
  await expect(page.getByTestId("journal-track")).not.toBeChecked();
  await expect(page.getByTestId("journal-entry")).toHaveCount(0);
});

test("a tracked write is listed with its masked URL, and reading requests are not", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await trackOn(page);
  await expect(page.getByTestId("journal-empty")).toContainText("Nothing written yet");
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await request(page, "List users");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await openJournal(page);
  await expect(page.getByTestId("journal-entry")).toHaveCount(1);
  const entry = page.getByTestId("journal-entry");
  await expect(entry).toContainText("POST");
  await expect(entry).toContainText("/users");
  await expect(entry).toContainText("Local");
  await expect(entry).toContainText("id mock-1");
  await expect(page.getByTestId("journal-count")).toContainText("1 entry");
  // Clearing forgets it.
  await page.getByTestId("journal-clear").click();
  await expect(page.getByTestId("journal-entry")).toHaveCount(0);
});

test("Revert shows the plan and Enter sends it; the entry is marked reverted", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await trackOn(page);
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await makeRemoveRequest(page);
  await openJournal(page);
  await page.getByTestId("journal-revert").click();
  await expect(page.getByTestId("journal-revert-dialog")).toBeVisible();
  await page.getByTestId("journal-pick").selectOption({ label: "Health" });
  await page.getByTestId("journal-plan-button").click();
  const plan = page.getByTestId("journal-plan");
  await expect(plan).toBeVisible();
  await expect(page.getByTestId("journal-plan-url")).toContainText("/users/mock-1");
  await expect(page.getByTestId("journal-plan-effect")).toContainText("writes");
  await expect(page.getByTestId("journal-plan-env")).toContainText("Local");
  await expect(plan).toHaveAttribute("data-production", "false");
  // Enter sends (the button has the focus).
  await expect(page.getByTestId("journal-send")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("journal-outcome")).toContainText("Reverted");
  await expect(page.getByTestId("journal-entry")).toHaveAttribute("data-reverted", "true");
  await expect(page.getByTestId("journal-revert")).toHaveCount(0);
});

test("a request that needs an id the entry does not have is refused with the reason", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("Local");
  await openJournal(page);
  // Tracking on, but no id path: the entry has no id.
  await page.getByTestId("journal-track").check();
  await request(page, "Create user");
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toBeVisible();
  await makeRemoveRequest(page);
  await openJournal(page);
  await page.getByTestId("journal-revert").click();
  await page.getByTestId("journal-pick").selectOption({ label: "Health" });
  await page.getByTestId("journal-plan-button").click();
  await expect(page.getByTestId("journal-plan-error")).toContainText("{{journal.id}}");
  await expect(page.getByTestId("journal-plan")).toHaveCount(0);
});

test("production asks 'Revert N changes in PRODUCTION?'; Esc sends nothing, Enter reverts", async ({ page }) => {
  await openDemo(page);
  await page.getByTestId("env-switcher").selectOption("PRD");
  await trackOn(page);
  await request(page, "Create user");
  await page.getByTestId("send").click();
  // The send itself asks once (wave 17).
  await expect(page.getByTestId("confirm-send")).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("response-status")).toBeVisible();
  await makeRemoveRequest(page);
  await openJournal(page);
  await expect(page.getByTestId("journal-entry")).toContainText("PRD");
  await page.getByTestId("journal-revert").click();
  await page.getByTestId("journal-pick").selectOption({ label: "Health" });
  await page.getByTestId("journal-plan-button").click();
  await expect(page.getByTestId("journal-plan")).toHaveAttribute("data-production", "true");
  await page.getByTestId("journal-send").click();
  const question = page.getByTestId("journal-production");
  await expect(question).toContainText("Revert 1 change in PRODUCTION?");
  await page.keyboard.press("Escape");
  await expect(question).toBeHidden();
  await expect(page.getByTestId("journal-outcome")).toHaveCount(0);
  await expect(page.getByTestId("journal-entry")).toHaveAttribute("data-reverted", "false");
  await page.getByTestId("journal-send").click();
  await expect(question).toBeVisible();
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("journal-outcome")).toContainText("Reverted");
  await expect(page.getByTestId("journal-entry")).toHaveAttribute("data-reverted", "true");
});
