// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte()],
  build: { target: "es2022" },
  server: { port: 5173, strictPort: true },
});
