// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The RequestTailor Authors

// Web shell entry. `?backend=mock` uses the in-memory MockBackend (UI tests; `&welcome=1`
// adds a bundled plugin and the first-start question); otherwise the real WebBackend
// (rt-wasm). Owner of the real backend: wave 3, strand E.

import { MockBackend, startApp, type AppExtension, type Backend } from "@requesttailor/ui";

import { createWebBackend } from "./web-backend";

const params = new URLSearchParams(location.search);
const backend: Backend =
  params.get("backend") === "mock" ? new MockBackend({ welcome: params.get("welcome") === "1" }) : await createWebBackend();

// Exposed for Playwright assertions only.
(window as unknown as { __rt: { backend: Backend } }).__rt = { backend };

// The demo extension exists only in the dev server (`?extensions=demo`); a build drops this branch.
const extensions: AppExtension[] = [];
if (import.meta.env.DEV && params.get("extensions") === "demo") {
  extensions.push((await import("./dev-extensions")).demoExtension);
}

startApp(document.getElementById("app")!, backend, { extensions });
