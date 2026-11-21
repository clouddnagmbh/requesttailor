// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand D's tests on their own port, so they can run next to other strands' dev servers:
// `npx playwright test --config e2e/environments/playwright.environments.config.ts`.

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = 5184;
const url = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  ...base,
  testDir: ".",
  use: { ...base.use, baseURL: url },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    url,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
