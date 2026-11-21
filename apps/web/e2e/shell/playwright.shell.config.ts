// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Runs only the shell tests on their own port, so they can run next to other strands' dev
// servers: `npx playwright test --config e2e/shell/playwright.shell.config.ts` (in apps/web).
// CI runs them through the base config together with all other folders.

import { defineConfig } from "@playwright/test";

import base from "../../playwright.config";

const PORT = 5181;
const url = `http://127.0.0.1:${PORT}`;

export default defineConfig({
  ...base,
  testDir: ".",
  use: { ...base.use, baseURL: url },
  webServer: {
    command: `npm run dev -- --host 127.0.0.1 --port ${PORT}`,
    cwd: "../..",
    url,
    reuseExistingServer: !process.env.CI,
    timeout: 60_000,
  },
});
