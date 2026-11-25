// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 19, strand G: the browser half of the budget `sequence_render`, run by
// `scripts/perf/sequence.mjs`: `npx playwright test --config e2e/sequence/sequence.perf.config.ts`
// (in apps/web). Mock backend only, so it measures the view. One worker. Port:
// RT_PERF_SEQUENCE_PORT (Vite, default 48731).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

const WEB_PORT = Number(process.env.RT_PERF_SEQUENCE_PORT ?? 48731);
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "perf.measure.ts",
  workers: 1,
  fullyParallel: false,
  reporter: "list",
  timeout: 180_000,
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true, viewport: { width: 1280, height: 800 } },
  webServer: {
    command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
    cwd: webRoot,
    url: `http://127.0.0.1:${WEB_PORT}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
