// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Strand G (companion) on its own port, so it does not collide with other dev servers:
//   npx playwright test --config e2e/companion/playwright.companion.config.ts
// The specs also run under the base config (`npm run e2e`). The real end-to-end test against
// `rt agent` needs RT_BIN and RT_TESTSERVER_BIN (see real-agent.spec.ts), else it is skipped.

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = 5187;

export default defineConfig({
  ...base,
  testDir: ".",
  use: { ...base.use, baseURL: `http://127.0.0.1:${PORT}` },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
