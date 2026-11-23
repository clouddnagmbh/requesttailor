// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Wave 15, strand C: the view part of the budget `git_graph_large`, run by
// `scripts/perf/run.mjs --only graph-large` (scripts/perf/graph-large.mjs): `npx playwright test
// --config e2e/perf/graph-large.perf.config.ts` (in apps/web). Mock backend with a synthetic
// repository of 100 000 commits (`?graph=100000`), so it measures the view, not Git. One worker,
// so nothing else competes for the CPU. Port: RT_PERF_GRAPH_LARGE_PORT (Vite, default 48414).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

const WEB_PORT = Number(process.env.RT_PERF_GRAPH_LARGE_PORT ?? 48414);
const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "graph-large.measure.ts",
  workers: 1,
  fullyParallel: false,
  reporter: "list",
  timeout: 180_000,
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true, viewport: { width: 1440, height: 900 } },
  webServer: {
    command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
    cwd: webRoot,
    url: `http://127.0.0.1:${WEB_PORT}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
