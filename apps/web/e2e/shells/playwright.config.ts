// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Shell tests: the real WebBackend (rt-wasm, File System Access via OPFS) against rt-testserver.
// Run: `npm run e2e:shells -w apps/web` (builds the core first). Test files end in `.e2e.ts`,
// so the base config (mock UI tests) never picks them up.

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

import { SERVER_PORT, WEB_PORT } from "./env";

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
      reuseExistingServer: !process.env.CI,
      timeout: 300_000,
    },
    {
      // Vite directly: the core is built by `npm run e2e:shells` before Playwright starts.
      command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
      cwd: webRoot,
      url: `http://127.0.0.1:${WEB_PORT}`,
      reuseExistingServer: !process.env.CI,
      timeout: 60_000,
    },
  ],
});
