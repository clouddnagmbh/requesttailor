// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 3, strand B (request editor), against the mock backend.

import { expect, test, type Locator, type Page } from "@playwright/test";

import type { Backend, RequestSpec, StoredResponse } from "@requesttailor/ui";

type Rt = { backend: Backend & { files: Map<string, string> }; lastSend?: StoredResponse };
declare global {
  interface Window {
    __rt: Rt;
  }
}

async function openRequest(page: Page, name: string, before?: () => Promise<void>) {
  await page.goto("/?backend=mock");
  if (before) await before();
  await page.getByTestId("open-collection").click();
  await page.getByTestId("tree-request").filter({ hasText: name }).click();
  await expect(page.getByTestId("request-editor")).toBeVisible();
}

/** The CodeMirror editor inside a field wrapper. */
const cm = (field: Locator) => field.locator(".cm-content");

/** The text of a CodeMirror field, without its placeholder. */
const fieldText = (field: Locator) =>
  cm(field).evaluate((el) => {
    const clone = el.cloneNode(true) as HTMLElement;
    clone.querySelectorAll(".cm-placeholder").forEach((n) => n.remove());
    return clone.textContent ?? "";
  });

async function setText(field: Locator, text: string) {
  const content = cm(field);
  await content.click();
  await content.press("ControlOrMeta+a");
  if (text === "") await content.press("Backspace");
  else await content.page().keyboard.type(text);
}

/** The spec as the backend has it (after the editor's debounced `app.edit`). */
const backendSpec = (page: Page, path: string) =>
  page.evaluate((p) => window.__rt.backend.getRequest(p), path) as Promise<RequestSpec>;

async function expectSpec(page: Page, path: string, check: (s: RequestSpec) => unknown, expected: unknown) {
  await expect.poll(async () => check(await backendSpec(page, path))).toEqual(expected);
}

const LIST = "users/list-users.yml";
const CREATE = "users/create-user.yml";

test("changing method and URL makes the request dirty; save clears it", async ({ page }) => {
  await openRequest(page, "List users");
  await expect(page.getByTestId("save")).toBeDisabled();

  await page.getByTestId("method-select").selectOption("POST");
  await expect(page.getByTestId("save")).toBeEnabled();

  await setText(page.getByTestId("url-input"), "{{baseUrl}}/members");
  await expectSpec(page, LIST, (s) => [s.method, s.url], ["POST", "{{baseUrl}}/members"]);
  expect(await page.evaluate(() => window.__rt.backend.dirty())).toEqual([LIST]);

  await page.getByTestId("save").click();
  await expect(page.getByTestId("save")).toBeDisabled();
  expect(await page.evaluate(() => window.__rt.backend.dirty())).toEqual([]);
  const file = await page.evaluate((p) => window.__rt.backend.files.get(p), LIST);
  expect(file).toContain('"method": "POST"');
  expect(file).toContain('"url": "{{baseUrl}}/members"');

  // A custom method.
  await page.getByTestId("method-select").selectOption("custom");
  await page.getByTestId("method-custom").fill("PROPFIND");
  await expectSpec(page, LIST, (s) => s.method, "PROPFIND");
});

test("URL and query params stay in sync both ways", async ({ page }) => {
  await openRequest(page, "List users");
  const url = page.getByTestId("url-input");
  const rows = page.getByTestId("params-table").getByTestId("kv-row");
  const row = (i: number) => rows.nth(i);
  const rowText = async () =>
    (await rows.all()).length === 0
      ? []
      : Promise.all((await rows.all()).map(async (r) => `${await fieldText(r.getByTestId("kv-name"))}=${await fieldText(r.getByTestId("kv-value"))}`));

  await expect(rows).toHaveCount(1);

  // URL -> rows (repeated names, empty value, `?` and `&` inside a template).
  await setText(url, "{{baseUrl}}/users?page=2&size=10&size=&q={{a&b?c}}");
  await expect(rows).toHaveCount(4);
  expect(await rowText()).toEqual(["page=2", "size=10", "size=", "q={{a&b?c}}"]);

  // Row edit -> URL.
  await setText(row(0).getByTestId("kv-value"), "3");
  await expect(cm(url)).toHaveText("{{baseUrl}}/users?page=3&size=10&size=&q={{a&b?c}}");

  // Disable -> removed from the URL but kept as a row; enable -> back in place.
  await row(1).getByTestId("kv-enabled").uncheck();
  await expect(cm(url)).toHaveText("{{baseUrl}}/users?page=3&size=&q={{a&b?c}}");
  await expect(rows).toHaveCount(4);
  await expectSpec(page, LIST, (s) => s.params.filter((p) => !p.enabled).map((p) => p.name), ["size"]);
  await row(1).getByTestId("kv-enabled").check();
  await expect(cm(url)).toHaveText("{{baseUrl}}/users?page=3&size=10&size=&q={{a&b?c}}");

  // Editing the URL keeps a disabled row.
  await row(0).getByTestId("kv-enabled").uncheck();
  await setText(url, "{{baseUrl}}/users?x=1");
  await expect(rows).toHaveCount(2);
  await expect(row(0).getByTestId("kv-enabled")).not.toBeChecked();
  expect(await rowText()).toEqual(["page=3", "x=1"]);

  // Add and remove rows.
  await page.getByTestId("params-table").getByTestId("kv-add").click();
  await setText(row(2).getByTestId("kv-name"), "token");
  await setText(row(2).getByTestId("kv-value"), "{{nope}}");
  await expect(cm(url)).toHaveText("{{baseUrl}}/users?x=1&token={{nope}}");
  await row(1).getByTestId("kv-remove").click();
  await expect(cm(url)).toHaveText("{{baseUrl}}/users?token={{nope}}");
  await row(1).getByTestId("kv-remove").click();
  await expect(cm(url)).toHaveText("{{baseUrl}}/users");

  await expectSpec(page, LIST, (s) => [s.url, s.params], [
    "{{baseUrl}}/users",
    [{ name: "page", value: "3", kind: "query", enabled: false }],
  ]);
});

test("path params appear as rows with values", async ({ page }) => {
  await openRequest(page, "List users");
  await setText(page.getByTestId("url-input"), "{{baseUrl}}/users/:id?page=1");
  const rows = page.getByTestId("path-params-table").getByTestId("kv-row");
  await expect(rows).toHaveCount(1);
  await expect(cm(rows.nth(0).getByTestId("kv-name"))).toHaveText("id");
  await setText(rows.nth(0).getByTestId("kv-value"), "42");
  await expectSpec(page, LIST, (s) => s.params, [
    { name: "page", value: "1", kind: "query", enabled: true },
    { name: "id", value: "42", kind: "path", enabled: true },
  ]);
});

test("headers: add, edit, remove, and they are sent", async ({ page }) => {
  await openRequest(page, "Health");
  await page.evaluate(() => {
    const b = window.__rt.backend;
    const send = b.send.bind(b);
    b.send = async (...args) => (window.__rt.lastSend = await send(...args));
  });
  const sentHeaders = () => page.evaluate(() => window.__rt.lastSend?.report.request?.headers ?? []);
  const send = async () => {
    await page.evaluate(() => (window.__rt.lastSend = undefined));
    await page.getByTestId("send").click();
    await expect(page.getByTestId("response-status")).toHaveText("200");
  };

  await page.getByTestId("tab-headers").click();
  const table = page.getByTestId("headers-table");
  await table.getByTestId("kv-add").click();
  const row = table.getByTestId("kv-row").nth(0);
  await setText(row.getByTestId("kv-name"), "X-Trace");
  await setText(row.getByTestId("kv-value"), "abc");
  await send();
  expect(await sentHeaders()).toContainEqual(["X-Trace", "abc"]);

  await setText(row.getByTestId("kv-value"), "def");
  await send();
  expect(await sentHeaders()).toContainEqual(["X-Trace", "def"]);

  await row.getByTestId("kv-enabled").uncheck();
  await send();
  expect((await sentHeaders()).map(([n]) => n)).not.toContain("X-Trace");

  await row.getByTestId("kv-enabled").check();
  await row.getByTestId("kv-remove").click();
  await expect(table.getByTestId("kv-row")).toHaveCount(0);
  await send();
  expect((await sentHeaders()).map(([n]) => n)).not.toContain("X-Trace");
  await expectSpec(page, "health.yml", (s) => s.headers, []);
});

test("JSON body edited in CodeMirror is stored in the spec", async ({ page }) => {
  await openRequest(page, "Create user");
  await page.getByTestId("tab-body").click();
  await expect(page.getByTestId("body-type")).toHaveValue("json");
  const editor = page.getByTestId("body-editor");
  await expect(cm(editor)).toContainText('"name": "Ada"');

  await setText(editor, '{"name": "Grace", "id": {{userId}}}');
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "raw",
    kind: "json",
    data: '{"name": "Grace", "id": {{userId}}}',
  });

  // Switching the type keeps the old body until saved; switching back restores it.
  await page.getByTestId("body-type").selectOption("form-url-encoded");
  await expectSpec(page, CREATE, (s) => s.body, { type: "form-url-encoded", fields: [] });
  await page.getByTestId("body-type").selectOption("json");
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "raw",
    kind: "json",
    data: '{"name": "Grace", "id": {{userId}}}',
  });
  // Raw kinds share the text.
  await page.getByTestId("body-type").selectOption("xml");
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "raw",
    kind: "xml",
    data: '{"name": "Grace", "id": {{userId}}}',
  });
});

test("form, multipart and file bodies", async ({ page }) => {
  await openRequest(page, "Create user");
  await page.getByTestId("tab-body").click();

  await page.getByTestId("body-type").selectOption("form-url-encoded");
  const form = page.getByTestId("form-table");
  await form.getByTestId("kv-add").click();
  await setText(form.getByTestId("kv-row").nth(0).getByTestId("kv-name"), "a");
  await setText(form.getByTestId("kv-row").nth(0).getByTestId("kv-value"), "1");
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "form-url-encoded",
    fields: [{ name: "a", value: "1", enabled: true }],
  });

  await page.getByTestId("body-type").selectOption("multipart");
  const mp = page.getByTestId("multipart-table");
  await mp.getByTestId("kv-add").click();
  const part = mp.getByTestId("kv-row").nth(0);
  await setText(part.getByTestId("kv-name"), "upload");
  await part.getByTestId("part-kind").selectOption("file");
  await setText(part.getByTestId("kv-value"), "files/a.png");
  await setText(part.getByTestId("part-content-type"), "image/png");
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "multipart",
    parts: [{ name: "upload", kind: "file", values: ["files/a.png"], content_type: "image/png", enabled: true }],
  });

  await page.getByTestId("body-type").selectOption("file");
  await setText(page.getByTestId("body-file-path"), "fixtures/user.json");
  await setText(page.getByTestId("body-file-content-type"), "application/json");
  await expectSpec(page, CREATE, (s) => s.body, {
    type: "file",
    path: "fixtures/user.json",
    content_type: "application/json",
  });

  await page.getByTestId("body-type").selectOption("none");
  await expectSpec(page, CREATE, (s) => s.body, { type: "none" });
});

test("bearer auth is set via the Auth tab", async ({ page }) => {
  await openRequest(page, "List users");
  await page.getByTestId("tab-auth").click();
  await expect(page.getByTestId("auth-type")).toHaveValue("inherit");
  await page.getByTestId("auth-type").selectOption("bearer");
  await setText(page.getByTestId("auth-token"), "{{token}}");
  await expectSpec(page, LIST, (s) => s.auth, { type: "bearer", token: "{{token}}" });

  await page.getByTestId("auth-type").selectOption("api-key");
  await setText(page.getByTestId("auth-key"), "X-Api-Key");
  await setText(page.getByTestId("auth-value"), "k");
  await page.getByTestId("auth-placement").selectOption("query");
  await expectSpec(page, LIST, (s) => s.auth, { type: "api-key", key: "X-Api-Key", value: "k", placement: "query" });

  // Back to bearer: the token typed before is still there (kept until saved).
  await page.getByTestId("auth-type").selectOption("bearer");
  await expectSpec(page, LIST, (s) => s.auth, { type: "bearer", token: "{{token}}" });
});

test("unsupported auth is read-only and explained", async ({ page }) => {
  await openRequest(page, "List users", () =>
    page.evaluate(async (p) => {
      const b = window.__rt.backend;
      await b.openCollection();
      const s = await b.getRequest(p);
      await b.updateRequest(p, { ...s, auth: { type: "unsupported", kind: "oauth2" } });
    }, LIST),
  );
  await page.getByTestId("tab-auth").click();
  await expect(page.getByTestId("auth-unsupported")).toContainText("oauth2");
  await expect(page.getByTestId("auth-type")).toBeDisabled();
  // Replacing it is an explicit second step.
  await page.getByTestId("auth-replace").click();
  await page.getByTestId("auth-type").selectOption("none");
  await expectSpec(page, LIST, (s) => s.auth, { type: "none" });
});

test("variables are highlighted with their provenance", async ({ page }) => {
  await openRequest(page, "Create user", () =>
    page.evaluate(() => window.__rt.backend.secrets.set("Local", "token", "s3cr3t-value")),
  );
  const url = page.getByTestId("url-input");
  const baseUrl = url.locator('.rt-var[data-var="baseUrl"]');
  await expect(baseUrl).toHaveAttribute("data-var-state", "resolved");
  await baseUrl.hover();
  const tip = page.getByTestId("var-tooltip").first();
  await expect(tip).toContainText("environment Local");
  await expect(tip).toContainText("http://127.0.0.1:47811");

  await page.getByTestId("tab-auth").click();
  const token = page.getByTestId("auth-token").locator('.rt-var[data-var="token"]');
  await expect(token).toHaveAttribute("data-var-state", "secret");
  await token.hover();
  await expect(tip).toContainText("secret");
  await expect(tip).not.toContainText("s3cr3t");
  await expect(page.getByTestId("request-editor")).not.toContainText("s3cr3t");

  await page.getByTestId("tab-headers").click();
  const table = page.getByTestId("headers-table");
  await table.getByTestId("kv-add").click();
  await setText(table.getByTestId("kv-row").nth(0).getByTestId("kv-name"), "X-Nope");
  await setText(table.getByTestId("kv-row").nth(0).getByTestId("kv-value"), "{{nope}}");
  const nope = table.locator('.rt-var[data-var="nope"]');
  await expect(nope).toHaveAttribute("data-var-state", "unresolved");
  await nope.hover();
  await expect(tip).toContainText("unresolved");

  // Keyboard: the tooltip also shows when the cursor is inside a placeholder.
  await cm(url).click();
  await cm(url).press("Home");
  await cm(url).press("ArrowRight");
  await expect(tip).toContainText("environment Local");

  // Changing the environment refreshes the provenance.
  await page.getByTestId("env-switcher").selectOption("PRD");
  await baseUrl.hover();
  await expect(tip).toContainText("environment PRD");
  await expect(tip).toContainText("https://api.example.com");
});

test("a very long URL scrolls inside its field; Send stays visible at the end of the row", async ({ page }) => {
  for (const width of [1280, 900]) {
    await page.setViewportSize({ width, height: 800 });
    await openRequest(page, "List users");
    const long = `{{baseUrl}}/search?${Array.from({ length: 40 }, (_, i) => `filter${i}=value-${i}`).join("&")}`;
    await setText(page.getByTestId("url-input"), long);
    const send = page.getByTestId("send");
    await expect(send).toBeVisible();
    const box = (await send.boundingBox())!;
    const urlbar = (await page.getByTestId("url-input").boundingBox())!;
    expect(box.x + box.width).toBeLessThanOrEqual(width);
    expect(urlbar.x + urlbar.width).toBeLessThanOrEqual(box.x + 1);
    const scrollWidth = await page.evaluate(() => document.documentElement.scrollWidth);
    expect(scrollWidth).toBeLessThanOrEqual(width);
  }
});

// The editor fields put their tooltip containers into <body> (one per field). They must take no
// room: with the editor's border on them they once stacked up into a band over the header.
test("editor fields leave nothing over the header", async ({ page }) => {
  await openRequest(page, "Create user");
  for (const tab of [/^Headers/, /^Body/, /^Scripts/]) await page.getByRole("tab", { name: tab }).first().click();
  const covered = await page.evaluate(() => {
    const header = document.querySelector("header")!;
    const width = document.documentElement.clientWidth;
    const out: string[] = [];
    for (const x of [5, Math.round(width / 2), width - 5]) {
      for (const y of [1, 3, 6]) {
        const top = document.elementFromPoint(x, y);
        if (top && !header.contains(top)) out.push(`${x},${y}: ${top.tagName.toLowerCase()}.${String(top.className)}`);
      }
    }
    const extra = [...document.body.children].filter((c) => c.getBoundingClientRect().height > 0 && !c.contains(header));
    return [...out, ...extra.map((c) => `body child ${c.tagName.toLowerCase()} ${c.getBoundingClientRect().height}px`)];
  });
  expect(covered).toEqual([]);
});
