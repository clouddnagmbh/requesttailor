// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Pull-request review against the real web backend (rt-wasm, isomorphic-git on an OPFS folder
// behind a fake folder picker), a real `rt agent` and the fake Git host
// (testdata/review/fake-host.mjs):
//   cargo build -p rt-cli -p rt-testserver && node scripts/build-wasm.mjs
//   RT_BIN=target/debug/rt npx playwright test --config e2e/review/playwright.real.config.ts
// (in apps/web). Ports (strand E, 48040-48049): web 48045 (RT_SHELLS_WEB_PORT), rt-testserver
// 48043 (RT_SHELLS_SERVER_PORT), fake GitHub 48041, fake GitLab 48042, rt agent 48044.
// Skipped without RT_BIN.

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

export const WEB_PORT = Number(process.env.RT_SHELLS_WEB_PORT ?? 48045);
export const SERVER_PORT = Number(process.env.RT_SHELLS_SERVER_PORT ?? 48043);

const webRoot = fileURLToPath(new URL("../..", import.meta.url));
const { RT_TESTSERVER_BIN } = process.env;
const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: false,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true },
  webServer: [
    {
      command: RT_TESTSERVER_BIN
        ? `${RT_TESTSERVER_BIN} --port ${SERVER_PORT}`
        : `cargo run -p rt-testserver --locked -- --port ${SERVER_PORT}`,
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
