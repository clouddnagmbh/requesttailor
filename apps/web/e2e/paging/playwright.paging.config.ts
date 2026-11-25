// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand C's Playwright config: the base config on its own port (RT_WEB_PORT, default 48592).
//   npx playwright test -c e2e/paging/playwright.paging.config.ts

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = Number(process.env.RT_WEB_PORT ?? 48592);

export default defineConfig({
  ...base,
  testDir: ".",
  workers: 2,
  use: { ...base.use, baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    ...base.webServer,
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    url: `http://127.0.0.1:${PORT}`,
  },
});
