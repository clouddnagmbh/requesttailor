// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Secret rescue against the real web backend (rt-wasm, isomorphic-git on an OPFS folder behind
// a fake folder picker): `node scripts/build-wasm.mjs && npx playwright test --config
// e2e/rescue/playwright.real.config.ts` (in apps/web). Port: RT_SHELLS_WEB_PORT (default 48061).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

export const WEB_PORT = Number(process.env.RT_SHELLS_WEB_PORT ?? 48061);

const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: false,
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
