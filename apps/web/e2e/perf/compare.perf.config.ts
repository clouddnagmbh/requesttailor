// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 14, strand H: the comparison view's budget `compare_view`, run by `scripts/perf/run.mjs
// --only compare` (scripts/perf/compare.mjs): `npx playwright test --config
// e2e/perf/compare.perf.config.ts` (in apps/web). Mock backend only, so it measures the view,
// not a network. One worker, so nothing else competes for the CPU. Port: RT_PERF_COMPARE_PORT
// (Vite, default 48274).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

const WEB_PORT = Number(process.env.RT_PERF_COMPARE_PORT ?? 48274);
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "compare.measure.ts",
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
