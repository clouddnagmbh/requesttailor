// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Frontend of the desktop app. `tauri dev` starts this on 127.0.0.1:1420 (tauri.conf.json
// `devUrl`); `tauri build` embeds `dist/`.

import { svelte } from "@sveltejs/vite-plugin-svelte";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [svelte()],
  // Tauri shows its own errors in the terminal; keep Vite's output readable next to it.
  clearScreen: false,
  build: { target: "es2022", outDir: "dist", emptyOutDir: true },
  server: { host: "127.0.0.1", port: 1420, strictPort: true },
});
