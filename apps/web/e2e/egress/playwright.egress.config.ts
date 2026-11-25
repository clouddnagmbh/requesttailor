// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Egress for extensions against the real web backend (rt-wasm, an OPFS folder behind a fake
// folder picker) and a real `rt agent`, started by the test (needs RT_BIN):
//   node scripts/build-wasm.mjs && RT_BIN=$PWD/target/debug/rt \
//     npx playwright test --config e2e/egress/playwright.egress.config.ts   (in apps/web)
// Ports: RT_SHELLS_WEB_PORT (default 48708); the fake target 48706 and the agent 48707 are in
// the test (RT_EGRESS_TARGET_PORT, RT_EGRESS_AGENT_PORT).

import { fileURLToPath } from "node:url";

import { defineConfig } from "@playwright/test";

export const WEB_PORT = Number(process.env.RT_SHELLS_WEB_PORT ?? 48708);

const webRoot = fileURLToPath(new URL("../..", import.meta.url));

export default defineConfig({
  testDir: ".",
  testMatch: "**/*.e2e.ts",
  fullyParallel: false,
  reporter: process.env.CI ? "github" : "list",
  use: { baseURL: `http://127.0.0.1:${WEB_PORT}`, channel: "chrome", headless: true },
  webServer: {
    command: `npx vite --host 127.0.0.1 --port ${WEB_PORT} --strictPort`,
    cwd: webRoot,
    url: `http://127.0.0.1:${WEB_PORT}`,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
