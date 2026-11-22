// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// The reopen-at-start flow against the real web backend (rt-wasm, OPFS, IndexedDB) and
// rt-testserver: `npx playwright test --config e2e/recent/playwright.recent.config.ts` (in
// apps/web, after `node scripts/build-wasm.mjs`). Ports: RT_RECENT_WEB_PORT (default 47843),
// RT_SHELLS_SERVER_PORT (rt-testserver, default 47844 here). Runs in CI's perf job.

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

// Read by ../shells/env.ts in the test workers (they inherit this process's environment).
const SERVER_PORT = Number((process.env.RT_SHELLS_SERVER_PORT ??= "47844"));
const WEB_PORT = Number(process.env.RT_RECENT_WEB_PORT ?? 47843);
const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: true,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true },
  webServer: [
    {
      command: `cargo run -p rt-testserver --locked -- --port ${SERVER_PORT}`,
      cwd: repoRoot,
      url: `http://127.0.0.1:${SERVER_PORT}/status/200`,
      reuseExistingServer: false,
      timeout: 300_000,
    },
    {
      command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
      cwd: webRoot,
      url: `http://127.0.0.1:${WEB_PORT}`,
      reuseExistingServer: false,
      timeout: 60_000,
    },
  ],
});
