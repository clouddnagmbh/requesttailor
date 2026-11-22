// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Import against the real web backend (rt-wasm, OPFS behind a fake folder picker):
// `node scripts/build-wasm.mjs && npx playwright test --config e2e/import/playwright.import.config.ts`
// (in apps/web). Port: RT_SHELLS_WEB_PORT (default 5197). No test server: importing sends nothing.

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

const WEB_PORT = Number(process.env.RT_SHELLS_WEB_PORT ?? 5197);
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: true,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true },
  webServer: {
    command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
    cwd: webRoot,
    url: `http://127.0.0.1:${WEB_PORT}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
