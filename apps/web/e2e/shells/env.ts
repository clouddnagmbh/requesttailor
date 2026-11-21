// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shared setup for the shell tests: ports, the sample collection, and a fake folder picker
// that hands the app a folder in the origin-private file system (OPFS) instead of a real one.

import type { Page } from "@playwright/test";

/** Vite dev server for these tests (strand E's port). */
export const WEB_PORT = 5185;
/** rt-testserver for these tests (47811 belongs to the conformance suite). */
export const SERVER_PORT = 47815;
export const SERVER = `http://127.0.0.1:${SERVER_PORT}`;

/** A small collection: one request, one environment pointing at rt-testserver, plus files
 *  that must be skipped when reading the folder. */
export function sampleCollection(): Record<string, string> {
  return {
    "opencollection.yml": 'opencollection: "1.0.0"\ninfo:\n  name: Shell test\n',
    "environments/Local.yml": `name: Local\nvariables:\n  - name: baseUrl\n    value: ${SERVER}\n  - secret: true\n    name: token\n`,
    "health.yml": [
      "info:",
      "  name: Health",
      "  type: http",
      "  seq: 1",
      "",
      "http:",
      "  method: GET",
      '  url: "{{baseUrl}}/echo/health"',
      "",
    ].join("\n"),
    "blocked.yml": [
      "info:",
      "  name: Refused",
      "  type: http",
      "  seq: 2",
      "",
      "http:",
      "  method: GET",
      // Port 9 (discard) is closed: fetch rejects with a TypeError, as for CORS.
      '  url: "http://127.0.0.1:9/nothing"',
      "",
    ].join("\n"),
    ".git/config": "[core]\n",
    "node_modules/pkg/x.yml": "x: 1\n",
    ".DS_Store": "x",
  };
}

/** Before any page script runs: `showDirectoryPicker` returns the OPFS folder `name`. */
export async function fakePicker(page: Page, name = "collection"): Promise<void> {
  await page.addInitScript((folder) => {
    Object.defineProperty(window, "showDirectoryPicker", {
      configurable: true,
      value: async () => (await navigator.storage.getDirectory()).getDirectoryHandle(folder, { create: true }),
    });
  }, name);
}

/** Writes `files` into the OPFS folder `name` (call after `page.goto`). */
export async function fillFolder(page: Page, files: Record<string, string>, name = "collection"): Promise<void> {
  await page.evaluate(
    async ({ files, name }) => {
      const root = await (await navigator.storage.getDirectory()).getDirectoryHandle(name, { create: true });
      for (const [path, text] of Object.entries(files)) {
        const parts = path.split("/");
        let dir = root;
        for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p, { create: true });
        const w = await (await dir.getFileHandle(parts[parts.length - 1], { create: true })).createWritable();
        await w.write(text);
        await w.close();
      }
    },
    { files, name },
  );
}

/** Reads a file from the OPFS folder `name`, or `null` if it does not exist. */
export async function readFolderFile(page: Page, path: string, name = "collection"): Promise<string | null> {
  return page.evaluate(
    async ({ path, name }) => {
      try {
        let dir = await (await navigator.storage.getDirectory()).getDirectoryHandle(name);
        const parts = path.split("/");
        for (const p of parts.slice(0, -1)) dir = await dir.getDirectoryHandle(p);
        return await (await (await dir.getFileHandle(parts[parts.length - 1])).getFile()).text();
      } catch {
        return null;
      }
    },
    { path, name },
  );
}

