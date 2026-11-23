// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 3, strand A (UI shell): tree, tabs, command palette, shortcuts,
// theme, and the keyboard-only path from app start to a sent request. Mock backend only.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";

async function openCollection(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const treeRequest = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name });
const tab = (page: Page, name: string) => page.getByTestId("tab").filter({ hasText: name });

test("tree shows folders and requests and opens a tab", async ({ page }) => {
  await openCollection(page);
  const tree = page.getByRole("tree", { name: "Requests" });
  await expect(tree.getByTestId("tree-folder")).toHaveText([/demo/, /users/]);
  await expect(tree.getByTestId("tree-request")).toHaveCount(5);
  await expect(treeRequest(page, "List users")).toContainText("GET");
  await expect(treeRequest(page, "Create user")).toContainText("POST");

  await treeRequest(page, "List users").click();
  await expect(tab(page, "List users")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("request-editor")).toContainText("{{baseUrl}}/users");

  // Collapse "users" from the keyboard, open "Health" with Enter.
  await page.getByTestId("tree-folder").filter({ hasText: "users" }).click();
  await expect(treeRequest(page, "Create user")).toBeHidden();
  await page.keyboard.press("ArrowRight");
  await expect(treeRequest(page, "Create user")).toBeVisible();
  await page.keyboard.press("ArrowLeft");
  await expect(treeRequest(page, "Create user")).toBeHidden();
  await page.keyboard.press("ArrowUp");
  await expect(treeRequest(page, "Health")).toBeFocused();
  await page.keyboard.press("Enter");
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("tab")).toHaveCount(2);
});

test("create a request in a folder via the UI", async ({ page }) => {
  await openCollection(page);
  await page.getByRole("button", { name: "New request in users" }).click();
  const name = page.getByTestId("new-request-name");
  await expect(name).toBeFocused();
  await name.fill("Delete user");
  await name.press("Enter");

  // It sits inside the "users" folder (the tree is a flat list of rows with aria-level).
  const created = treeRequest(page, "Delete user");
  await expect(created).toContainText("GET");
  await expect(created).toHaveAttribute("data-path", "users/delete-user.yml");
  await expect(created).toHaveAttribute("aria-level", "2");
  await expect(created).toHaveAttribute("aria-selected", "true");
  await expect(tab(page, "Delete user")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("new-request-name")).toHaveCount(0);

  // Mod+N opens the "New…" dialog (wave 14): Escape closes it; Enter takes the HTTP tile, then
  // the name and Enter create the request at the root.
  await page.keyboard.press(`${MOD}+n`);
  await expect(page.getByTestId("new-filter")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("new-dialog")).toHaveCount(0);
  await page.keyboard.press(`${MOD}+n`);
  await page.keyboard.press("Enter");
  await page.keyboard.type("Ping");
  await page.keyboard.press("Enter");
  await expect(treeRequest(page, "Ping")).toHaveAttribute("data-path", "ping.yml");
  await expect(treeRequest(page, "Ping")).toHaveAttribute("aria-level", "1");
  await expect(tab(page, "Ping")).toHaveAttribute("aria-selected", "true");
});

test("tabs switch, close and show the dirty marker", async ({ page }) => {
  await openCollection(page);
  await treeRequest(page, "List users").click();
  await treeRequest(page, "Create user").click();
  await treeRequest(page, "Health").click();
  await expect(page.getByTestId("tab")).toHaveText([/List users/, /Create user/, /Health/]);
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");

  await tab(page, "List users").click();
  await expect(tab(page, "List users")).toHaveAttribute("aria-selected", "true");
  await expect(page.getByTestId("request-editor")).toContainText("/users?page=1");

  // Close button, middle click, Mod+W.
  await tab(page, "Create user").getByRole("button", { name: "Close Create user" }).click();
  await expect(page.getByTestId("tab")).toHaveText([/List users/, /Health/]);
  await tab(page, "Health").click({ button: "middle" });
  await expect(page.getByTestId("tab")).toHaveText([/List users/]);
  await page.keyboard.press(`${MOD}+w`);
  await expect(page.getByTestId("tab")).toHaveCount(0);

  // A new request is unsaved: the tab shows the marker until Save.
  await page.getByRole("button", { name: "New request in users" }).click();
  await page.getByTestId("new-request-name").fill("Draft");
  await page.getByTestId("new-request-name").press("Enter");
  await expect(tab(page, "Draft").getByTestId("tab-dirty")).toBeVisible();
  await page.getByTestId("save").click();
  await expect(tab(page, "Draft").getByTestId("tab-dirty")).toHaveCount(0);
});

test("command palette opens with Mod+K, filters and runs send", async ({ page }) => {
  await openCollection(page);
  await treeRequest(page, "Health").click();
  await page.keyboard.press(`${MOD}+k`);
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  const input = palette.getByRole("combobox");
  await expect(input).toBeFocused();
  // All commands are listed, with their shortcuts.
  await expect(palette.getByRole("option").filter({ hasText: "Open folder" })).toBeVisible();
  await expect(palette.getByRole("option").filter({ hasText: "Environment: PRD" })).toBeVisible();

  await input.fill("sndreq");
  await expect(palette.getByRole("option")).toHaveCount(1);
  await expect(palette.getByRole("option").first()).toContainText("Send request");
  await input.press("Enter");
  await expect(palette).toBeHidden();
  await expect(page.getByTestId("response-status")).toHaveText("200");

  // Environment entries switch the environment.
  await page.keyboard.press(`${MOD}+k`);
  await page.keyboard.type("env prd");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("env-switcher")).toHaveValue("PRD");
  // Escape closes without running anything.
  await page.keyboard.press(`${MOD}+k`);
  await page.keyboard.press("Escape");
  await expect(palette).toBeHidden();
});

test("Mod+Enter sends and Mod+S saves with focus inside an input", async ({ page }) => {
  await openCollection(page);
  await page.getByRole("button", { name: "New request in users" }).click();
  await page.getByTestId("new-request-name").fill("Update user");
  await page.getByTestId("new-request-name").press("Enter");
  await expect(tab(page, "Update user").getByTestId("tab-dirty")).toBeVisible();

  // Stand-ins for the editor's fields: a plain input, and an editable element that swallows
  // keydown like CodeMirror does when it handles a key itself.
  await page.evaluate(() => {
    const main = document.querySelector("[data-testid=request-editor]")!;
    const input = document.createElement("input");
    input.dataset.testid = "probe-input";
    const editable = document.createElement("div");
    editable.contentEditable = "true";
    editable.dataset.testid = "probe-editable";
    editable.textContent = "body";
    editable.addEventListener("keydown", (e) => e.stopPropagation());
    main.append(input, editable);
  });

  await page.getByTestId("probe-input").focus();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  await page.keyboard.press(`${MOD}+s`);
  await expect(tab(page, "Update user").getByTestId("tab-dirty")).toHaveCount(0);
  await expect(page.getByTestId("save")).toBeDisabled();
  expect(await page.evaluate(() => [...(window as any).__rt.backend.files.keys()])).toContain("users/update-user.yml");

  // Same from inside the CodeMirror-like element.
  await page.getByRole("button", { name: "New request in users" }).click();
  await page.getByTestId("new-request-name").fill("Second");
  await page.getByTestId("new-request-name").press("Enter");
  await tab(page, "Update user").click();
  await page.getByTestId("probe-editable").focus();
  await page.keyboard.press(`${MOD}+s`);
  await expect(tab(page, "Second").getByTestId("tab-dirty")).toHaveCount(0);
  await expect(page.getByTestId("probe-editable")).toBeFocused();
});

test("theme toggle switches data-theme and persists across reload", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "light" });
  await page.goto("/?backend=mock");
  const html = page.locator("html");
  const toggle = page.getByTestId("theme-toggle");
  await expect(html).not.toHaveAttribute("data-theme", /./);
  await expect(toggle).toHaveAccessibleName(/system/i);

  await toggle.click();
  await expect(html).toHaveAttribute("data-theme", "light");
  await toggle.click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  const darkBg = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);

  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "dark");
  expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).toBe(darkBg);

  // The palette's "Toggle theme" flips between light and dark.
  await page.keyboard.press(`${MOD}+k`);
  await page.keyboard.type("toggle theme");
  await page.keyboard.press("Enter");
  await expect(html).toHaveAttribute("data-theme", "light");
  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "light");

  // Back to the OS preference: the attribute goes away.
  await toggle.click(); // light -> dark
  await toggle.click(); // dark -> system
  await expect(html).not.toHaveAttribute("data-theme", /./);
  await page.reload();
  await expect(html).not.toHaveAttribute("data-theme", /./);
});

test("keyboard only: from app start to a sent request", async ({ page }) => {
  let keystrokes = 0;
  const press = async (key: string) => {
    keystrokes++;
    await page.keyboard.press(key);
  };

  // First start on this device: open, pick the request by type-ahead, open it, send.
  await page.goto("/?backend=mock");
  await press(`${MOD}+o`);
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
  await expect(page.getByRole("tree").getByRole("treeitem").first()).toBeFocused();
  await press("h");
  await expect(treeRequest(page, "Health")).toBeFocused();
  await press("Enter");
  await press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  expect(keystrokes).toBe(4);
  test.info().annotations.push({ type: "keystrokes (first start)", description: String(keystrokes) });

  // Later starts: the shell restores the tabs of the collection, so open + send is enough.
  keystrokes = 0;
  await page.reload();
  await press(`${MOD}+o`);
  await expect(tab(page, "Health")).toHaveAttribute("aria-selected", "true");
  await press(`${MOD}+Enter`);
  await expect(page.getByTestId("response-status")).toHaveText("200");
  expect(keystrokes).toBe(2);
  test.info().annotations.push({ type: "keystrokes (later starts)", description: String(keystrokes) });
});
