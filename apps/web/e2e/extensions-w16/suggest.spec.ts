// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 16, strand C: editor actions with `EditorHandle.propose` (inline diff, Apply, Discard,
// undo in the editor) and `ctx.suggest` for each kind of edit (dialog, Apply, undo), against the
// mock backend with the demo16 extension of the dev server.

import { expect, test } from "@playwright/test";

import { demoLog, editorText, MOD, openCollection, openRequest } from "./helpers";

async function openScripts(page: import("@playwright/test").Page) {
  await page.getByTestId("tab-scripts").click();
  await expect(page.getByTestId("script-before-request")).toBeVisible();
}

test("an editor action proposes an inline diff; Apply is one undo step of the editor", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await openScripts(page);
  const editor = page.getByTestId("script-before-request").locator(".cm-content");
  await editor.click();
  await page.keyboard.type("test('ok', () => {});");
  await expect.poll(() => editorText(page, "script-before-request")).toContain("test('ok'");

  await page.getByTestId("editor-shell").first().getByRole("button", { name: "Add logging" }).first().click();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toBeVisible();
  await expect(view).toContainText("Log when the script ends");
  await expect(view.getByTestId("suggest-added")).toHaveText("+1");
  await expect(view.locator('[data-kind="added"]')).toContainText('console.log("done")');
  // The text is untouched until Apply.
  await expect(page.getByTestId("script-before-request")).toBeHidden();

  await view.getByTestId("suggest-apply").click();
  await expect(view).toHaveCount(0);
  await expect.poll(() => editorText(page, "script-before-request")).toContain('console.log("done")');
  // The edit reached the request: the Scripts tab keeps it.
  await expect(page.getByTestId("tab-scripts")).toContainText("1");
  expect(await demoLog(page)).toContain("propose-script:true");

  // One step back in the editor's own history restores the text.
  await page.getByTestId("script-before-request").locator(".cm-content").click();
  await page.keyboard.press(`${MOD}+z`);
  await expect.poll(() => editorText(page, "script-before-request")).not.toContain("console.log");
  expect(await editorText(page, "script-before-request")).toContain("test('ok'");
});

test("Discard and Escape keep the text; the shortcut runs the action", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await openScripts(page);
  await page.getByTestId("script-before-request").locator(".cm-content").click();
  await page.keyboard.type("// mine");
  await page.keyboard.press(`${MOD}+Shift+l`);
  const view = page.getByTestId("suggested-edit");
  await expect(view).toBeVisible();
  await view.getByTestId("suggest-discard").click();
  await expect(view).toHaveCount(0);
  expect(await editorText(page, "script-before-request")).toContain("// mine");
  expect(await editorText(page, "script-before-request")).not.toContain("console.log");
  expect(await demoLog(page)).toContain("propose-script:false");

  // Escape discards too; the menu action wraps the script.
  await page.getByTestId("editor-actions-menu").first().click();
  await page.getByRole("menuitem", { name: "Wrap in try/catch" }).click();
  await expect(view).toBeVisible();
  await expect(view.locator('[data-kind="added"]').first()).toContainText("try {");
  await page.keyboard.press("Escape");
  await expect(view).toHaveCount(0);
  expect(await demoLog(page)).toEqual(["propose-script:false", "propose-wrap:false"]);
});

test("the body editor offers its own action", async ({ page }) => {
  await openCollection(page);
  await openRequest(page, "Create user");
  await page.getByTestId("tab-body").click();
  await expect(page.getByTestId("body-editor")).toBeVisible();
  await page.getByRole("button", { name: "Compact JSON" }).click();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toBeVisible();
  await view.getByTestId("suggest-apply").click();
  await expect(view).toHaveCount(0);
  expect(await editorText(page, "body-editor")).not.toContain("\n");
});

test("a request suggestion: diff in a dialog, Apply makes an unsaved edit, Undo puts it back", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await page.getByTestId("tab-headers").click();
  const rowsBefore = await page.getByTestId("tab-headers").innerText();

  await page.locator('[data-action="demo16/header"]').click();
  const dialog = page.getByTestId("extension-dialog");
  const view = dialog.getByTestId("suggested-edit");
  await expect(view).toBeVisible();
  await expect(view).toContainText("Trace this request");
  await expect(view.getByTestId("suggest-block").first()).toContainText("Header › X-Request-Id");
  await expect(view.locator('[data-kind="added"]').first()).toContainText("{{requestId}}");
  await expect(view.getByTestId("suggest-apply")).toBeFocused();

  await page.keyboard.press(`${MOD}+Enter`);
  await expect(dialog).toHaveCount(0);
  expect(await demoLog(page)).toContain("suggest-request:true");
  await expect(page.getByTestId("suggest-toast")).toContainText("Suggestion applied");
  await expect(page.getByTestId("tab-headers")).not.toHaveText(rowsBefore);
  await expect(page.getByTestId("save")).toBeEnabled();

  await page.getByTestId("suggest-undo").click();
  await expect(page.getByTestId("suggest-toast")).toContainText("Suggestion undone");
  await expect(page.getByTestId("tab-headers")).toHaveText(rowsBefore);
});

test("discarding a request suggestion changes nothing", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await page.locator('[data-action="demo16/header"]').click();
  await page.getByTestId("suggest-discard").click();
  await expect(page.getByTestId("extension-dialog")).toHaveCount(0);
  expect(await demoLog(page)).toContain("suggest-request:false");
  await expect(page.getByTestId("suggest-toast")).toHaveCount(0);
  await expect(page.getByTestId("save")).toBeDisabled();
});

test("a new request and a file are suggested from the More menu", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  await page.getByTestId("extension-actions-menu").first().click();
  await page.getByRole("menuitem", { name: "Suggest a health check" }).click();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toContainText("A request to check the service is up");
  await expect(view).toContainText("Discard it with the other unsaved changes");
  await view.getByTestId("suggest-apply").click();
  await expect(page.getByTestId("tab").filter({ hasText: "Health check" })).toHaveCount(1);
  await expect(page.getByTestId("suggest-toast")).not.toContainText("Undo");
  await expect(page.getByTestId("suggest-undo")).toHaveCount(0);

  await page.getByTestId("extension-actions-menu").first().click();
  await page.getByRole("menuitem", { name: "Suggest release notes" }).click();
  await expect(view).toContainText("RELEASE-NOTES.md");
  await expect(view.getByTestId("suggest-block")).toContainText("new");
  await view.getByTestId("suggest-apply").click();
  expect(await page.evaluate(() => (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend.companions.read("RELEASE-NOTES.md"))).toContain("Added a trace header");
  expect(await demoLog(page)).toEqual(expect.arrayContaining(["suggest-new:true", "suggest-file:true"]));
});

test("folder scripts are suggested, applied unsaved, and undone", async ({ page }) => {
  await openCollection(page);
  await openRequest(page);
  const scripts = () =>
    page.evaluate(() => (window as unknown as { __rt: { backend: import("@requesttailor/ui").Backend } }).__rt.backend.folderScripts.get("users"));
  expect(await scripts()).toEqual([]);
  await page.getByTestId("extension-actions-menu").first().click();
  await page.getByRole("menuitem", { name: "Suggest folder scripts" }).click();
  const view = page.getByTestId("suggested-edit");
  await expect(view).toContainText("Set a trace id for every request in users");
  await expect(view.getByTestId("suggest-block")).toContainText("traceId");
  await view.getByTestId("suggest-apply").click();
  expect(await scripts()).toEqual([{ phase: "before-request", code: "rt.vars.set('traceId', 'demo');\n" }]);
  await expect(page.getByTestId("save")).toBeEnabled();
  await page.getByTestId("suggest-undo").click();
  expect(await scripts()).toEqual([]);
  expect(await demoLog(page)).toContain("suggest-folder-scripts:true");
});

// A suggestion with many blocks scrolls; no block is squeezed below its own content (the list is
// a scrolling flex column, whose children shrank to their label once there were more than five).
test("a suggestion with many blocks keeps every block whole and scrolls", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 600 });
  await openCollection(page);
  await openRequest(page);
  await page.getByTestId("extension-actions-menu").first().click();
  await page.getByRole("menuitem", { name: "Suggest a health check" }).click();
  const view = page.getByTestId("suggested-edit");
  const blocks = view.getByTestId("suggest-block");
  expect(await blocks.count()).toBeGreaterThan(5);
  const squeezed = await blocks.evaluateAll((els) =>
    els.filter((el) => el.getBoundingClientRect().height + 1 < el.scrollHeight).map((el) => el.textContent?.slice(0, 40)),
  );
  expect(squeezed).toEqual([]);
  const list = blocks.first().locator("xpath=..");
  expect(await list.evaluate((el) => el.scrollHeight > el.clientHeight)).toBe(true);
});
