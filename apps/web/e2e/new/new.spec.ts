// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Acceptance tests of wave 14, strand B: the "New…" dialog creates every built-in kind with the
// keyboard only (Mod+N, a tile by arrow keys or typing, Enter, the form, Mod+Enter), unsaved and
// in the chosen target; Escape goes back and closes; no horizontal scrolling at 900 px; a tile of
// an extension appears with its badge. Mock backend; `harness.html` adds a test extension and
// answers `collections.create`.

import { expect, test, type Page } from "@playwright/test";

const MOD = "ControlOrMeta";
const HARNESS = "/e2e/new/harness.html";

async function openCollection(page: Page, url = "/?backend=mock") {
  await page.goto(url);
  await page.getByTestId("open-collection").click();
  await expect(page.getByTestId("collection-name")).toHaveText("Demo API");
}

const dialog = (page: Page) => page.getByTestId("new-dialog");
const tile = (page: Page, key: string) => page.locator(`[data-testid=new-tile][data-key="${key}"]`);
const treeRequest = (page: Page, name: string) => page.getByTestId("tree-request").filter({ hasText: name });

async function openNew(page: Page) {
  await page.keyboard.press(`${MOD}+n`);
  await expect(dialog(page)).toBeVisible();
  await expect(page.getByTestId("new-filter")).toBeFocused();
}

test("Mod+N, arrow keys and Enter create every kind of request", async ({ page }) => {
  await openCollection(page);
  const kinds: [key: string, steps: number, name: string, method: string][] = [
    ["http", 0, "Get orders", "POST"],
    ["graphql", 1, "Search", "POST"],
    ["grpc", 2, "Say hello", "GRPC"],
    ["websocket", 3, "Echo socket", "WS"],
    ["sse", 4, "Ticker", "GET"],
  ];
  for (const [key, steps, name, method] of kinds) {
    await openNew(page);
    await expect(tile(page, "http")).toHaveAttribute("data-current", "true");
    for (let i = 0; i < steps; i++) await page.keyboard.press("ArrowRight");
    await expect(tile(page, key)).toHaveAttribute("data-current", "true");
    await page.keyboard.press("Enter");
    await expect(page.getByTestId("new-form")).toHaveAttribute("data-kind", key);
    // The name starts selected: typing replaces the default.
    await expect(page.getByTestId("new-name")).toBeFocused();
    await page.keyboard.type(name);
    await page.keyboard.press("Tab");
    if (key === "http") {
      await expect(page.getByTestId("new-method")).toBeFocused();
      await page.keyboard.press("p"); // POST
      await page.keyboard.press("Tab");
    }
    await expect(page.getByTestId("new-url")).toBeFocused();
    await page.keyboard.type(`{{baseUrl}}/${key}`);
    await page.keyboard.press(`${MOD}+Enter`);
    await expect(dialog(page)).toBeHidden();

    // Unsaved, opened in a tab, at the top level (the dialog was opened for the root).
    const row = treeRequest(page, name);
    await expect(row).toContainText(method);
    await expect(row).toHaveAttribute("aria-level", "1");
    await expect(page.getByTestId("tab").filter({ hasText: name })).toHaveAttribute("aria-selected", "true");
    await expect(page.getByTestId("tab").filter({ hasText: name }).getByTestId("tab-dirty")).toBeVisible();
  }
  const specs = await page.evaluate(async () => {
    const b = (window as any).__rt.backend;
    return Promise.all(["get-orders.yml", "ticker.yml", "echo-socket.yml"].map((p) => b.getRequest(p)));
  });
  expect(specs[0]).toMatchObject({ method: "POST", url: "{{baseUrl}}/http" });
  expect(specs[1].headers).toContainEqual({ name: "Accept", value: "text/event-stream", enabled: true });
  expect(specs[2]).toMatchObject({ method: "WS", protocol: { type: "websocket" } });
});

test("typing filters the tiles; a request goes into the chosen folder", async ({ page }) => {
  await openCollection(page);
  await openNew(page);
  await page.keyboard.type("graph");
  await expect(tile(page, "graphql")).toHaveAttribute("data-current", "true");
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Backspace");
  await page.keyboard.press("Backspace");
  await page.keyboard.type("zzzz");
  await expect(page.getByTestId("new-empty")).toContainText("Nothing matches");
  if (process.env.RT_SHOTS) await page.screenshot({ path: `${process.env.RT_SHOTS}/new-empty.png`, animations: "disabled" });
  await page.getByTestId("new-filter").fill("");
  await page.keyboard.type("http");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Delete user");
  await page.getByTestId("new-folder").selectOption("users");
  await page.getByTestId("new-name").press("Enter");
  await expect(dialog(page)).toBeHidden();
  await expect(treeRequest(page, "Delete user")).toHaveAttribute("data-path", "users/delete-user.yml");
  await expect(treeRequest(page, "Delete user")).toHaveAttribute("aria-level", "2");
});

test("folder, environment and MCP test from the keyboard", async ({ page }) => {
  await openCollection(page);
  await page.evaluate(() => {
    const b = (window as any).__rt.backend;
    const original = b.createFolder.bind(b);
    b.folderCalls = [];
    b.createFolder = (path: string) => {
      b.folderCalls.push(path);
      return original(path);
    };
  });

  // Folder: an existing name is refused, a new one is created.
  await openNew(page);
  await page.keyboard.type("folder");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("new-form")).toHaveAttribute("data-kind", "folder");
  await page.keyboard.type("users");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(page.getByTestId("new-form")).toContainText("There is already a folder “users” here.");
  await page.getByTestId("new-name").fill("Orders");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(dialog(page)).toBeHidden();
  expect(await page.evaluate(() => (window as any).__rt.backend.folderCalls)).toEqual(["Orders"]);

  // Environment: a copy of Local, chosen at once.
  await openNew(page);
  await page.keyboard.type("env");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Staging");
  await page.getByTestId("new-copy-from").selectOption("Local");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(dialog(page)).toBeHidden();
  await expect(page.getByTestId("env-switcher")).toHaveValue("Staging");
  const staging = await page.evaluate(async () => {
    const envs = await (window as any).__rt.backend.environments();
    return envs.find((e: { name: string }) => e.name === "Staging");
  });
  expect(staging.variables.find((v: { secret: boolean }) => v.secret)).toMatchObject({ value: null });

  // MCP server test: staged, then the MCP client opens with it listed.
  await openNew(page);
  await page.keyboard.type("mcp");
  await page.keyboard.press("Enter");
  await page.keyboard.type("Echo tool");
  await page.getByTestId("new-url").fill("http://localhost:3001/mcp");
  await expect(page.getByRole("radio", { name: "Local command" })).toBeDisabled();
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(dialog(page)).toBeHidden();
  await expect(page.getByTestId("mcp-panel")).toBeVisible();
  await expect(page.getByTestId("mcp-test").filter({ hasText: "echo-tool" })).toBeVisible();
});

test("a collection goes into a folder named after it", async ({ page }) => {
  await openCollection(page, HARNESS);
  await openNew(page);
  await page.keyboard.type("collection");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("new-form")).toHaveAttribute("data-kind", "collection");
  await page.keyboard.type("Shop API");
  await expect(page.getByTestId("new-dir")).toHaveValue("shop-api");
  await expect(page.getByTestId("new-environment")).toHaveValue("Local");
  await page.getByTestId("new-base-url").fill("http://localhost:3000");
  await page.keyboard.press(`${MOD}+Enter`);
  await expect(dialog(page)).toBeHidden();
  await expect(page.getByTestId("collection-name")).toHaveText("Shop API");
  const created = await page.evaluate(() => (window as any).__rt.calls.created);
  expect(created).toEqual([
    { name: "Shop API", root: "mock-collection", dir: "shop-api", base_url: "http://localhost:3000", environment: "Local" },
  ]);
});

test("Escape goes back to the tiles, then closes; the palette opens it too", async ({ page }) => {
  await openCollection(page);
  await openNew(page);
  await page.keyboard.press("ArrowDown");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("new-form")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("new-form")).toHaveCount(0);
  await expect(page.getByTestId("new-filter")).toBeFocused();
  await page.keyboard.press("Escape");
  await expect(dialog(page)).toBeHidden();
  // Nothing was created; Mod+Enter on the page behind would have sent, not created.
  await expect(page.getByTestId("tab")).toHaveCount(0);

  await page.keyboard.press(`${MOD}+k`);
  await page.keyboard.type("New…");
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeVisible();
  await dialog(page).getByRole("button", { name: "Close" }).click();
  await expect(dialog(page)).toBeHidden();
});

test("a tile of an extension appears with its badge and runs for the target", async ({ page }) => {
  await openCollection(page, HARNESS);
  await openNew(page);
  const thing = tile(page, "test/thing");
  await expect(thing).toContainText("Test thing");
  await expect(thing.getByTestId("new-tile-badge")).toHaveText("Beta");
  await expect(page.getByRole("region", { name: "Extras" })).toContainText("Test thing");
  // Built-in tiles carry no badge.
  await expect(page.getByTestId("new-tile-badge")).toHaveCount(1);
  await page.keyboard.type("test thing");
  await expect(thing).toHaveAttribute("data-current", "true");
  if (process.env.RT_SHOTS) await page.screenshot({ path: `${process.env.RT_SHOTS}/new-extension-tile.png`, animations: "disabled" });
  await page.keyboard.press("Enter");
  await expect(dialog(page)).toBeHidden();
  expect(await page.evaluate(() => (window as any).__rt.calls.ran)).toEqual([{ collection: "mock-collection", folder: "" }]);
});

for (const width of [900, 1440]) {
  test(`no horizontal scrolling at ${width} px, light and dark`, async ({ page }) => {
    await page.setViewportSize({ width, height: 760 });
    await openCollection(page, HARNESS);
    for (const scheme of ["light", "dark"] as const) {
      await page.emulateMedia({ colorScheme: scheme });
      await openNew(page);
      const fits = () =>
        page.evaluate(() => {
          const d = document.querySelector<HTMLElement>("[data-testid=new-dialog]")!;
          // Visually hidden text (a live region) clips on purpose.
          const over = [...d.querySelectorAll<HTMLElement>("*")].filter(
            (el) => el.clientWidth > 1 && el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).overflowX !== "visible",
          );
          const r = d.getBoundingClientRect();
          return { inside: r.left >= 0 && r.right <= innerWidth, page: document.documentElement.scrollWidth <= innerWidth, over: over.map((e) => e.className) };
        });
      expect(await fits()).toEqual({ inside: true, page: true, over: [] });
      // Alignment: the filter and the tiles share the left edge; tiles in a row share top and width.
      const boxes = await page.evaluate(() => {
        const r = (el: Element) => el.getBoundingClientRect();
        const filter = r(document.querySelector("[data-testid=new-filter]")!.closest(".rt-field-box")!);
        const tiles = [...document.querySelectorAll("[data-testid=new-tile] .rt-card")].map(r);
        return { filter: { left: filter.left, right: filter.right }, tiles: tiles.map((t) => ({ left: t.left, top: t.top, width: t.width, right: t.right })) };
      });
      expect(Math.abs(boxes.filter.left - boxes.tiles[0].left)).toBeLessThan(1);
      const firstRow = boxes.tiles.filter((t) => Math.abs(t.top - boxes.tiles[0].top) < 1);
      expect(firstRow.length).toBeGreaterThan(1);
      for (const t of firstRow) expect(Math.abs(t.width - firstRow[0].width)).toBeLessThan(1);
      expect(Math.abs(firstRow[firstRow.length - 1].right - boxes.filter.right)).toBeLessThan(1);
      if (process.env.RT_SHOTS) await page.screenshot({ path: `${process.env.RT_SHOTS}/new-tiles-${width}-${scheme}.png`, animations: "disabled" });
      for (const kind of ["http", "collection", "mcp-test"]) {
        await page.getByTestId("new-filter").fill("");
        await tile(page, kind).click();
        await expect(page.getByTestId("new-form")).toHaveAttribute("data-kind", kind);
        expect(await fits(), kind).toEqual({ inside: true, page: true, over: [] });
        if (process.env.RT_SHOTS) await page.screenshot({ path: `${process.env.RT_SHOTS}/new-${kind}-${width}-${scheme}.png`, animations: "disabled" });
        await page.keyboard.press("Escape");
      }
      await page.keyboard.press("Escape");
      await expect(dialog(page)).toBeHidden();
    }
  });
}
