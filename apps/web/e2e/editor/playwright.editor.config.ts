// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand B (request editor) on its own port, so it can run next to other strands' servers:
//   npx playwright test --config e2e/editor/playwright.editor.config.ts   (in apps/web)
// The base config (`playwright.config.ts`) runs these tests too.

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = 5182;
const url = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  ...base,
  testDir: ".",
  use: { ...base.use, baseURL: url },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    cwd: fileURLToPath(new URL("../..", import.meta.url)),
    url,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
