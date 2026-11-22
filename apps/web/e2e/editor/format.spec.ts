// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// "Format" for request bodies (wave 8), against the mock backend: an explicit, undoable edit
// that keeps {{variables}} byte-identical; Shift+Alt+F does the same from anywhere.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Backend, RequestSpec } from "@requesttailor/ui";

const CREATE = "users/create-user.yml";
const cm = (field: Locator) => field.locator(".cm-content");

async function openCreateBody(page: Page) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Create user" }).click();
  await page.getByTestId("tab-body").click();
  await expect(page.getByTestId("body-type")).toHaveValue("json");
}

async function setText(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  // Inserted, not typed: the XML editor closes tags while typing.
  await content.page().keyboard.insertText(text);
}

async function expectData(page: Page, data: string) {
  await expect
    .poll(async () => {
      const spec = (await page.evaluate(
        (p) => (window as unknown as { __rt: { backend: Backend } }).__rt.backend.getRequest(p),
        CREATE,
      )) as RequestSpec;
      return spec.body.type === "raw" ? spec.body.data : null;
    })
    .toBe(data);
}

const COMPACT = '{"name":"Grace","id":{{userId}},"tags":["a",{{tag}}],"n":1.0}';
const PRETTY = '{\n  "name": "Grace",\n  "id": {{userId}},\n  "tags": [\n    "a",\n    {{tag}}\n  ],\n  "n": 1.0\n}';

test("Format pretty-prints a JSON body, keeps placeholders and is undoable", async ({ page }) => {
  await openCreateBody(page);
  const editor = page.getByTestId("body-editor");
  // Loading never reformats: the file's text is shown as it is.
  await expect(cm(editor)).toHaveText('{  "name": "Ada"}');
  await page.getByTestId("save").waitFor();
  await expect(page.getByTestId("save")).toBeDisabled();

  await setText(editor, COMPACT);
  await expectData(page, COMPACT);
  await page.getByTestId("body-format").click();
  await expectData(page, PRETTY);
  await expect(editor.locator(".cm-line")).toHaveCount(9);
  await expect(page.getByTestId("save")).toBeEnabled();

  // A second format changes nothing.
  await page.getByTestId("body-format").click();
  await expect(page.getByTestId("body-format-status")).toHaveText("Already formatted");

  // Undo restores the text as typed.
  await cm(editor).click();
  await page.keyboard.press("ControlOrMeta+z");
  await expectData(page, COMPACT);

  // The fold gutter folds the array.
  await page.getByTestId("body-format").click();
  await expectData(page, PRETTY);
  const gutter = (await editor.locator(".cm-foldGutter").boundingBox())!;
  const row = (await editor.locator(".cm-line").filter({ hasText: '"tags": [' }).boundingBox())!;
  await page.mouse.click(gutter.x + gutter.width / 2, row.y + row.height / 2);
  await expect(editor.locator(".cm-foldPlaceholder")).toHaveCount(1);
  await expectData(page, PRETTY);
});

test("Shift+Alt+F formats from another tab; malformed bodies stay as they are", async ({ page }) => {
  await openCreateBody(page);
  const editor = page.getByTestId("body-editor");
  await setText(editor, COMPACT);
  await expectData(page, COMPACT);
  await page.getByTestId("tab-headers").click();
  await page.keyboard.press("Shift+Alt+KeyF");
  await expect(page.getByTestId("body-editor")).toBeVisible();
  await expectData(page, PRETTY);

  await setText(editor, '{"a": ');
  await page.keyboard.press("Shift+Alt+KeyF");
  await expect(page.getByTestId("body-format-status")).toHaveText("Not well-formed JSON: nothing changed");
  await expectData(page, '{"a": ');
});

test("Format pretty-prints an XML body", async ({ page }) => {
  await openCreateBody(page);
  await page.getByTestId("body-type").selectOption("xml");
  const editor = page.getByTestId("body-editor");
  await setText(editor, '<user id="{{id}}"><name>Ada</name><!-- c --><tags/></user>');
  await page.getByTestId("body-format").click();
  await expectData(page, '<user id="{{id}}">\n  <name>Ada</name>\n  <!-- c -->\n  <tags/>\n</user>');
});

test("the palette offers Format body for JSON bodies", async ({ page }) => {
  await openCreateBody(page);
  await setText(page.getByTestId("body-editor"), '{"a":1}');
  await expectData(page, '{"a":1}');
  await page.keyboard.press("ControlOrMeta+k");
  await page.keyboard.type("Format body");
  await page.keyboard.press("Enter");
  await expectData(page, '{\n  "a": 1\n}');
});
