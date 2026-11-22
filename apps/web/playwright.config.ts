// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// UI tests against the web shell. Each strand keeps its tests in `e2e/<strand>/`.
// Uses the installed Google Chrome (`channel: "chrome"`), so no browser download is needed.

import { defineConfig } from "@playwright/test";

// RT_WEB_PORT: run on another port (parallel worktrees must not reuse someone else's server).
const port = Number(process.env.RT_WEB_PORT ?? 5173);

export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${port}`, channel: "chrome", headless: true },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${port} --strictPort`,
    url: `http://127.0.0.1:${port}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
