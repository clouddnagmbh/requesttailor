// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand C's Playwright config: the base config on its own port, so it can run next to the
// other strands' dev servers. From apps/web:
//   npx playwright test -c e2e/response/playwright.response.config.ts

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = 5183;

export default defineConfig({
  ...base,
  testDir: ".",
  use: { ...base.use, baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    ...base.webServer,
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    url: `http://127.0.0.1:${PORT}`,
  },
});
