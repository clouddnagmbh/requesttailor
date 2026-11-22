// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Branch compare and bisect against the real web backend (rt-wasm, isomorphic-git on an OPFS
// folder behind a fake folder picker) and rt-testserver:
// `node scripts/build-wasm.mjs && npx playwright test --config e2e/compare/playwright.real.config.ts`
// (in apps/web). Ports: RT_SHELLS_WEB_PORT (default 48022), RT_SHELLS_SERVER_PORT (default
// 48021).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

export const WEB_PORT = Number(process.env.RT_SHELLS_WEB_PORT ?? 48022);
export const SERVER_PORT = Number(process.env.RT_SHELLS_SERVER_PORT ?? 48021);

const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: false,
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
