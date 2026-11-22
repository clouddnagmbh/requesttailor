// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Frame-time measurements (budget `large_response_frame`), run by `scripts/perf/run.mjs`:
// `npx playwright test --config e2e/perf/playwright.perf.config.ts` (in apps/web, after
// `node scripts/build-wasm.mjs`). One worker, so nothing else competes for the CPU.
// Ports: RT_PERF_WEB_PORT (Vite, default 47841), RT_SHELLS_SERVER_PORT (rt-testserver,
// default 47842 here).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

// Read by ../shells/env.ts in the test workers (they inherit this process's environment).
const SERVER_PORT = Number((process.env.RT_SHELLS_SERVER_PORT ??= "47842"));
const WEB_PORT = Number(process.env.RT_PERF_WEB_PORT ?? 47841);
const repoRoot = fileURLToPath(new URL("../../../..", import.meta.url));
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.perf.ts",
  workers: 1,
  fullyParallel: false,
  reporter: "list",
  timeout: 120_000,
  // The window size of a laptop screen; the viewport decides how many rows render.
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true, viewport: { width: 1280, height: 800 } },
  webServer: [
    {
      // Release build: the debug server is slow to stream 100 MB.
      command: `cargo run --release -p rt-testserver --locked -- --port ${SERVER_PORT}`,
      cwd: repoRoot,
      url: `http://127.0.0.1:${SERVER_PORT}/status/200`,
      reuseExistingServer: false,
      timeout: 600_000,
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
