// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pretty view of response bodies (wave 8): pretty-printed, highlighted by content type and
// foldable and searchable, against the mock backend (`/json-compact`, `/xml`, `/html`, `/css`
// answer fixed bodies).

import { expect, test, type Locator, type Page } from "@playwright/test";

async function sendTo(page: Page, path: string) {
  await page.goto("/?backend=mock");
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: "Health" }).click();
  const url = page.getByTestId("url-input").locator(".cm-content");
  await url.click();
  await url.press("ControlOrMeta+a");
  await page.keyboard.type(`{{baseUrl}}/${path}`);
  await page.getByTestId("send").click();
  await expect(page.getByTestId("response-status")).toHaveText("200");
}

const pretty = (page: Page) => page.getByTestId("response-pretty");
const lines = (view: Locator) => view.locator(".cm-line");
const line = (view: Locator, text: string) => lines(view).filter({ hasText: text });

/** Clicks the fold marker in the gutter next to `row`. */
async function clickFoldMarker(view: Locator, row: Locator) {
  const gutter = (await view.locator(".cm-foldGutter").boundingBox())!;
  const r = (await row.boundingBox())!;
  await view.page().mouse.click(gutter.x + gutter.width / 2, r.y + r.height / 2);
}

test("JSON is pretty-printed as received, highlighted and folds by mouse and keyboard", async ({ page }) => {
  await sendTo(page, "json-compact");
  await expect(page.getByTestId("response-mode-pretty")).toHaveAttribute("aria-pressed", "true");
  const view = pretty(page);
  await expect(view).toHaveAttribute("data-language", "json");
  await expect(lines(view).first()).toHaveText("{");
  // Numbers keep their text: no parse/stringify round trip.
  await expect(line(view, '"id": 12345678901234567890,')).toHaveCount(1);
  await expect(line(view, '"price": 1.0,')).toHaveCount(1);
  await expect(line(view, '"empty": {}')).toHaveCount(1);
  // Highlighted: the key and the string value are coloured differently.
  const colors = await line(view, '"name": "a"')
    .locator("span")
    .evaluateAll((spans) => new Set(spans.map((s) => getComputedStyle(s).color)).size);
  expect(colors).toBeGreaterThanOrEqual(2);

  // Fold the "items" array with the gutter marker, unfold with the placeholder.
  await clickFoldMarker(view, line(view, '"items": ['));
  await expect(view.locator(".cm-foldPlaceholder")).toHaveCount(1);
  await expect(line(view, '"name": "a"')).toHaveCount(0);
  await view.locator(".cm-foldPlaceholder").click();
  await expect(view.locator(".cm-foldPlaceholder")).toHaveCount(0);
  await expect(line(view, '"name": "a"')).toHaveCount(1);

  // Keyboard: Mod-Alt-[ folds at the cursor, Mod-Alt-] unfolds.
  await line(view, '"items": [').click();
  await page.keyboard.press("ControlOrMeta+Alt+BracketLeft");
  await expect(line(view, '"name": "b"')).toHaveCount(0);
  await page.keyboard.press("ControlOrMeta+Alt+BracketRight");
  await expect(line(view, '"name": "b"')).toHaveCount(1);

  // The view is read-only.
  await page.keyboard.type("x");
  await expect(line(view, '"items": [')).toHaveText('  "items": [');

  // Raw stays the body as received.
  await page.getByTestId("response-mode-raw").click();
  await expect(page.getByTestId("response-lines")).toContainText('{"id":12345678901234567890,"price":1.0,');
});

test("XML is pretty-printed, highlighted and folds elements", async ({ page }) => {
  await sendTo(page, "xml");
  await expect(page.getByTestId("response-mode-pretty")).toHaveAttribute("aria-pressed", "true");
  await expect(page.getByTestId("response-mode-tree")).toBeDisabled();
  await expect(page.getByTestId("response-filter")).toHaveCount(0);
  const view = pretty(page);
  await expect(view).toHaveAttribute("data-language", "xml");
  await expect(lines(view)).toHaveText([
    '<?xml version="1.0"?>',
    '<users count="2">',
    "  <!-- page 1 -->",
    '  <user id="1">',
    "    <name>Ada</name>",
    "    <tags>",
    "      <tag>admin</tag>",
    "    </tags>",
    "  </user>",
    '  <user id="2">',
    "    <name>Grace</name>",
    "    <note><![CDATA[a < b]]></note>",
    "  </user>",
    "</users>",
  ]);

  await clickFoldMarker(view, line(view, '<user id="1">'));
  await expect(view.locator(".cm-foldPlaceholder")).toHaveCount(1);
  await expect(line(view, "<name>Ada</name>")).toHaveCount(0);
  await expect(line(view, "<name>Grace</name>")).toHaveCount(1);
  await clickFoldMarker(view, line(view, '<user id="1">'));
  await expect(line(view, "<name>Ada</name>")).toHaveCount(1);

  await clickFoldMarker(view, line(view, '<users count="2">'));
  await expect(lines(view)).toHaveCount(2);
});

test("HTML is highlighted and foldable but never reflowed", async ({ page }) => {
  await sendTo(page, "html");
  const view = pretty(page);
  await expect(view).toHaveAttribute("data-language", "html");
  await expect(line(view, "<pre>  keep")).toHaveCount(1);
  await expect(line(view, "   me</pre>")).toHaveCount(1);
  await expect(line(view, "<p>Hello<br>world</p>")).toHaveCount(1);
  await clickFoldMarker(view, line(view, "<head>"));
  await expect(line(view, "<title>Mock</title>")).toHaveCount(0);
});

test("CSS is highlighted and foldable", async ({ page }) => {
  await sendTo(page, "css");
  const view = pretty(page);
  await expect(view).toHaveAttribute("data-language", "css");
  const colors = await line(view, "color: #333;")
    .locator("span")
    .evaluateAll((spans) => new Set(spans.map((s) => getComputedStyle(s).color)).size);
  expect(colors).toBeGreaterThanOrEqual(2);
  await clickFoldMarker(view, line(view, "@media"));
  await expect(line(view, "display: none;")).toHaveCount(0);
});

test("Mod+F searches the pretty view: next, previous, Escape closes", async ({ page }) => {
  await sendTo(page, "json-compact");
  const view = pretty(page);
  await lines(view).first().click();
  await page.keyboard.press("ControlOrMeta+f");
  const panel = view.locator(".cm-search");
  await expect(panel).toBeVisible();
  // Read-only: no replace fields.
  await expect(panel.locator("input[name=replace]")).toHaveCount(0);
  await expect(panel.locator("input[name=search]")).toBeFocused();
  await page.keyboard.type('"name"');
  await page.keyboard.press("Enter");
  const selected = view.locator(".cm-searchMatch-selected");
  await expect(view.locator(".cm-searchMatch")).toHaveCount(2);
  await expect(line(view, '"name": "a"').locator(".cm-searchMatch-selected")).toHaveCount(1);
  await page.keyboard.press("Enter");
  await expect(line(view, '"name": "b"').locator(".cm-searchMatch-selected")).toHaveCount(1);
  await page.keyboard.press("Shift+Enter");
  await expect(line(view, '"name": "a"').locator(".cm-searchMatch-selected")).toHaveCount(1);
  await expect(selected).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(panel).toHaveCount(0);
});

test("Mod+F does not open a search panel in single-line fields", async ({ page }) => {
  await sendTo(page, "json-compact");
  await page.getByTestId("url-input").locator(".cm-content").click();
  await page.keyboard.press("ControlOrMeta+f");
  await expect(page.locator(".cm-search")).toHaveCount(0);
});

test("JSONPath results are highlighted JSON", async ({ page }) => {
  await sendTo(page, "json-compact");
  await page.getByTestId("response-filter").fill("$.items");
  const result = page.getByTestId("response-filter-result").getByTestId("response-filter-code");
  await expect(result).toHaveAttribute("data-language", "json");
  await expect(line(result, '"name": "a"')).toHaveCount(1);
});
